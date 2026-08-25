//! Visor interactivo del grafo agregado.
//!
//! Migrado desde bilinker: el traversal de cadenas era conocimiento de su
//! formato, pero renderizar el grafo nunca lo fue. Acá además puede mostrar
//! aristas de todos los proveedores, no solo las de bilinker.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use lattice::model::{Edge, Guarantee, NodeId};

pub fn esc_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
     .replace('\n', "\\n").replace('\r', "").replace('\t', "\\t")
}

fn layer_id(label: &str) -> String {
    format!("layer_{}", label.replace(['/', '.', '-'], "_"))
}

/// Profundidad de la capa, para ordenar las columnas: spec a la izquierda,
/// impl a la derecha.
fn layer_depth(label: &str) -> usize {
    if label == "." { 0 } else { label.matches('/').count() + 1 }
}

fn lang_from_file(file: &str) -> &'static str {
    match Path::new(file).extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs"   => "rust",
        "java" => "java",
        "md"   => "markdown",
        "yaml" | "yml" => "yaml",
        "ts" | "tsx" | "js" | "jsx" => "typescript",
        _ => "text",
    }
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() { return s.len(); }
    while i > 0 && !s.is_char_boundary(i) { i -= 1; }
    i
}

/// Contenido del fragmento y su línea inicial, para el panel de detalle.
fn fragment_of(base: &Path, n: &NodeId) -> (String, usize) {
    let Some((layer, path, range)) = n.as_fragment() else { return (String::new(), 1) };
    let abs = base.join(if layer == "." { "" } else { layer }).join(path);
    let Ok(src) = std::fs::read_to_string(&abs) else { return (String::new(), 1) };

    match range {
        None => (src, 1),
        Some((s, e)) => {
            let s = floor_char_boundary(&src, s.min(src.len()));
            let e = floor_char_boundary(&src, e.min(src.len()));
            let line = src[..s].chars().filter(|&c| c == '\n').count() + 1;
            (src[s..e.max(s)].to_string(), line)
        }
    }
}

pub fn render(base: &Path, edges: &[Edge]) -> String {
    // Nodos únicos, en orden estable.
    let mut ids: Vec<&NodeId> = edges.iter().flat_map(|e| [&e.from, &e.to]).collect();
    ids.sort();
    ids.dedup();

    let mut layers: BTreeMap<String, usize> = BTreeMap::new();
    for n in &ids {
        let l = n.as_fragment().map(|(l, _, _)| l).unwrap_or("externo").to_string();
        let d = layer_depth(&l);
        layers.insert(l, d);
    }

    let layers_json = layers.iter().map(|(lbl, depth)| {
        format!(r#"{{"id":"{}","label":"{}","depth":{}}}"#,
            esc_json(&layer_id(lbl)), esc_json(lbl), depth)
    }).collect::<Vec<_>>().join(",");

    let fg_id = |file: &str, layer: &str| {
        format!("fg_{}_{}", file.replace(['/', '.', '-'], "_"),
                            layer.replace(['/', '.', '-'], "_"))
    };

    // Orden: por profundidad de capa, luego directorio y archivo. Es lo que
    // hace que el visor se lea como el árbol del proyecto y no como una nube.
    let key = |n: &NodeId| {
        let (layer, path, range) = n.as_fragment()
            .map(|(l, p, r)| (l.to_string(), p.to_string(), r))
            .unwrap_or(("externo".into(), n.0.clone(), None));
        let dir  = Path::new(&path).parent().map(|p| p.display().to_string()).unwrap_or_default();
        let name = Path::new(&path).file_name().map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        (layer_depth(&layer), layer, dir, name, range.map(|(s, _)| s).unwrap_or(0), path)
    };
    let mut ordered = ids.clone();
    ordered.sort_by_key(|n| key(n));

    let mut fg_seen = HashSet::new();
    let mut fg_parts = Vec::new();
    for n in &ordered {
        let (_, layer, _, name, _, path) = key(n);
        let gid = fg_id(&path, &layer);
        if fg_seen.insert(gid.clone()) {
            fg_parts.push(format!(
                r#"{{"id":"{}","label":"{}","layer_id":"{}","layer":"{}","type":"file-group"}}"#,
                esc_json(&gid), esc_json(&name),
                esc_json(&layer_id(&layer)), esc_json(&layer)));
        }
    }

    let mut row: BTreeMap<String, usize> = BTreeMap::new();
    let mut prev_fg: BTreeMap<String, String> = BTreeMap::new();
    let nodes_json = ordered.iter().map(|n| {
        let (depth, layer, _, _, start, path) = key(n);
        let gid = fg_id(&path, &layer);
        let r = row.entry(layer.clone()).or_insert(0);
        let p = prev_fg.entry(layer.clone()).or_default();
        if !p.is_empty() && *p != gid { *r += 1; }
        *p = gid.clone();
        let y = *r;
        *r += 1;

        let (content, start_line) = fragment_of(base, n);
        let abs = base.join(if layer == "." { "" } else { &layer }).join(&path);
        format!(
            r#"{{"id":"{}","label":"{}","file_group_id":"{}","layer_id":"{}","layer":"{}","abs_path":"{}","content":"{}","start_line":{},"lang":"{}","xi":{},"yi":{}}}"#,
            esc_json(&n.0), esc_json(&format!("@{start}")),
            esc_json(&gid), esc_json(&layer_id(&layer)), esc_json(&layer),
            esc_json(&abs.display().to_string()), esc_json(&content),
            start_line, lang_from_file(&path), depth, y)
    }).collect::<Vec<_>>().join(",");

    // La garantía viaja al visor: sin ella, una inferencia del LSP se ve igual
    // que una referencia verificada.
    let edges_json = edges.iter().enumerate().map(|(i, e)| {
        let states = e.state.as_ref()
            .map(|[a, b]| format!("{a}↔{b}"))
            .unwrap_or_else(|| e.guarantee.to_string());
        format!(
            r#"{{"id":"e{i}","source":"{}","target":"{}","label":"{}","states":"{}","link0":"{}","link1":"{}"}}"#,
            esc_json(&e.from.0), esc_json(&e.to.0),
            esc_json(&format!("{} · {}", &e.r#ref[..8.min(e.r#ref.len())], e.kind)),
            esc_json(&states), esc_json(&e.from.0), esc_json(&e.to.0))
    }).collect::<Vec<_>>().join(",");

    let data = format!(
        r#"{{"layers":[{layers_json}],"file_groups":[{}],"nodes":[{nodes_json}],"edges":[{edges_json}]}}"#,
        fg_parts.join(","));
    let _ = Guarantee::Accepted;
    TEMPLATE.replace("GRAPH_DATA_PLACEHOLDER", &data)
}

const TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>Bilink Graph</title>
<link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/github-dark.min.css">
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: 'Courier New', monospace; display: flex; height: 100vh; overflow: hidden; background: #0d1117; color: #c9d1d9; }
#cy { flex: 1; background: #0d1117; }
#panel { width: 420px; padding: 20px; overflow-y: auto; background: #161b22; border-left: 2px solid #30363d; display: flex; flex-direction: column; gap: 12px; }
.hint     { color: #6e7681; font-size: 13px; }
.ntitle   { font-size: 14px; font-weight: bold; color: #58a6ff; word-break: break-all; }
.nlayer   { font-size: 11px; color: #8b949e; margin-top: 2px; }
.open-link { display: inline-block; padding: 5px 14px; background: #1f6feb; color: #fff; text-decoration: none; border-radius: 6px; font-size: 12px; margin-top: 4px; }
.open-link:hover { background: #388bfd; }

/* code view */
.code-wrap { border: 1px solid #30363d; border-radius: 6px; display: flex; max-height: 220px; overflow: hidden; }
.line-nums  { padding: 0.5em 0.6em; background: #161b22; border-right: 1px solid #30363d; text-align: right; color: #6e7681; font-size: 11px; line-height: 1.6; user-select: none; white-space: pre; flex-shrink: 0; overflow: hidden; }
.code-wrap pre  { margin: 0; padding: 0.5em 0; overflow: auto; font-size: 11px; line-height: 1.6; flex: 1; min-width: 0; }
.code-wrap code { display: block; white-space: pre; }

/* markdown view */
.md-wrap { background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 16px; overflow-y: auto; max-height: 220px; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; font-size: 13px; line-height: 1.7; color: #c9d1d9; }
.md-wrap h1,.md-wrap h2,.md-wrap h3,.md-wrap h4 { color: #58a6ff; margin: 16px 0 6px; }
.md-wrap h1 { font-size: 18px; border-bottom: 1px solid #30363d; padding-bottom: 6px; }
.md-wrap h2 { font-size: 15px; }
.md-wrap p  { margin: 8px 0; }
.md-wrap ul,.md-wrap ol { padding-left: 20px; margin: 6px 0; }
.md-wrap code { background: #161b22; padding: 2px 5px; border-radius: 3px; font-family: 'Courier New', monospace; font-size: 11px; color: #a5d6ff; }
.md-wrap pre  { background: #161b22; border: 1px solid #30363d; border-radius: 6px; padding: 12px; overflow-x: auto; margin: 8px 0; }
.md-wrap pre code { background: none; padding: 0; font-size: 11px; }
.md-wrap table { border-collapse: collapse; width: 100%; margin: 8px 0; font-size: 12px; }
.md-wrap td,.md-wrap th { border: 1px solid #30363d; padding: 5px 10px; }
.md-wrap th { background: #1c2938; color: #58a6ff; }
.md-wrap a  { color: #58a6ff; }
.md-wrap blockquote { border-left: 3px solid #30363d; padding-left: 12px; color: #8b949e; margin: 8px 0; }
.md-wrap hr { border: none; border-top: 1px solid #30363d; margin: 12px 0; }

/* bilink divider */
.bilink-sep { display: flex; align-items: center; gap: 10px; margin: 4px 0; }
.bilink-sep-line { flex: 1; border-top: 1px solid #30363d; }
.bilink-sep-label { font-size: 11px; color: #58a6ff; font-family: 'Courier New', monospace; white-space: nowrap; }
.frag-label { font-size: 11px; color: #8b949e; margin-bottom: 4px; }

/* garantía */
.sg-header { font-size: 12px; font-weight: bold; color: #8b949e; margin: 10px 0 4px; letter-spacing: 0.05em; }
.sg-list { display: flex; flex-direction: column; gap: 3px; }
.sg-item { display: flex; align-items: center; gap: 8px; font-size: 11px; font-family: 'Courier New', monospace; padding: 3px 6px; border-radius: 4px; background: #0d1117; }
.sg-sym  { flex: 1; color: #c9d1d9; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.sg-file { font-size: 10px; color: #6e7681; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 120px; }
.sg-badge { font-size: 10px; font-weight: bold; padding: 1px 6px; border-radius: 3px; white-space: nowrap; }
.sg-ok       { background: #1a3a1a; color: #3fb950; }
.sg-bad      { background: #3a1a1a; color: #f85149; }
.sg-restyled { background: #3a2e00; color: #d4a017; }
</style>
</head>
<body>
<div id="cy"></div>
<div id="panel"><div class="hint">← Click a node or edge to view details</div></div>
<script src="https://cdnjs.cloudflare.com/ajax/libs/cytoscape/3.28.1/cytoscape.min.js"></script>
<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>
<script src="https://cdnjs.cloudflare.com/ajax/libs/marked/9.1.6/marked.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js"></script>
<script>
mermaid.initialize({ startOnLoad: false, theme: 'dark' });

// Override marked renderer to render mermaid blocks as diagrams
const renderer = new marked.Renderer();
const origCode = renderer.code.bind(renderer);
renderer.code = function(code, lang) {
  if (lang === 'mermaid') {
    return `<div class="mermaid">${code}</div>`;
  }
  return origCode(code, lang);
};
marked.use({ renderer });

const G   = GRAPH_DATA_PLACEHOLDER;
const COL = 540, ROW = 90;
const elements = [];

G.layers.forEach(l =>
  elements.push({ data: { id: l.id, label: l.label, type: 'layer' } })
);

G.file_groups.forEach(fg =>
  elements.push({ data: { id: fg.id, label: fg.label, parent: fg.layer_id, type: 'file-group' } })
);

// Group layers by depth and assign vertical bands
const layerOrder = {};
const depthGroups = {};
G.layers.forEach(l => {
  if (!depthGroups[l.depth]) depthGroups[l.depth] = [];
  depthGroups[l.depth].push(l.id);
});

// Count nodes per layer to compute band heights
const nodesPerLayer = {};
G.nodes.forEach(n => { nodesPerLayer[n.layer_id] = (nodesPerLayer[n.layer_id] || 0) + 1; });

// Assign a y-band start offset to each layer within its depth column
const layerYStart = {};
Object.keys(depthGroups).sort().forEach(depth => {
  let offset = 0;
  depthGroups[depth].forEach(lid => {
    layerYStart[lid] = offset;
    offset += ((nodesPerLayer[lid] || 1) + 2) * ROW; // +2 rows gap between layers
  });
});

const yIdx = {};
G.nodes.forEach(n => {
  const k = n.layer_id;
  yIdx[k] = yIdx[k] || 0;
  const y = (layerYStart[n.layer_id] || 0) + yIdx[k]++ * ROW;
  elements.push({
    data: { id: n.id, label: n.label, parent: n.file_group_id, type: 'file',
            abs_path: n.abs_path, content: n.content, layer: n.layer,
            start_line: n.start_line, lang: n.lang,
          },
    position: { x: n.xi * COL, y: y }
  });
});

G.edges.forEach(e =>
  elements.push({ data: { id: e.id, source: e.source, target: e.target,
                          label: e.label + '\n' + e.states,
                          uuid: e.label, states: e.states,
                          link0: e.link0, link1: e.link1,
                          type: 'bilink-edge' } })
);

const cy = cytoscape({
  container: document.getElementById('cy'),
  elements,
  style: [
    { selector: 'node[type="layer"]', style: {
        'background-color': 'rgba(255,255,255,0.03)', 'background-opacity': 1,
        'border-color': '#30363d', 'border-style': 'dashed', 'border-width': 2,
        'label': 'data(label)', 'text-valign': 'top', 'text-halign': 'center',
        'color': '#6e7681', 'font-family': 'Courier New', 'font-size': 12, 'padding': 28 }},
    { selector: 'node[type="file-group"]', style: {
        'background-color': 'rgba(31,111,235,0.07)', 'background-opacity': 1,
        'border-color': '#1f6feb', 'border-style': 'solid', 'border-width': 1,
        'label': 'data(label)', 'text-valign': 'top', 'text-halign': 'center',
        'color': '#58a6ff', 'font-family': 'Courier New', 'font-size': 10, 'padding': 14 }},
    { selector: 'node[type="file"]', style: {
        'shape': 'round-rectangle', 'background-color': '#161b22',
        'border-color': '#1f6feb', 'border-width': 1.5,
        'label': 'data(label)', 'text-valign': 'center',
        'color': '#c9d1d9', 'font-family': 'Courier New', 'font-size': 11,
        'padding': 10, 'width': 'label', 'cursor': 'pointer' }},
    { selector: 'node[type="file"]:selected', style: {
        'border-color': '#58a6ff', 'border-width': 2.5, 'background-color': '#1c2938' }},
    { selector: 'edge', style: {
        'curve-style': 'bezier', 'target-arrow-shape': 'triangle', 'source-arrow-shape': 'triangle',
        'label': 'data(label)', 'color': '#8b949e', 'font-family': 'Courier New', 'font-size': 9,
        'text-background-color': '#0d1117', 'text-background-opacity': 0.85, 'text-background-padding': 3,
        'line-color': '#30363d', 'target-arrow-color': '#30363d', 'source-arrow-color': '#30363d',
        'width': 1.5, 'text-wrap': 'wrap' }},
    { selector: 'edge:selected', style: {
        'line-color': '#1f6feb', 'target-arrow-color': '#1f6feb', 'source-arrow-color': '#1f6feb' }}
  ],
  layout: { name: 'preset' }
});

cy.fit(undefined, 40);


function stateBadge(state, ok) {
  if (ok)                    return `<span class="sg-badge sg-ok">OK</span>`;
  if (state === 'RESTYLED')  return `<span class="sg-badge sg-restyled">RESTYLED</span>`;
  return `<span class="sg-badge sg-bad">${esc(state)}</span>`;
}

  const badge = stateBadge(n.state, n.ok);
  const txt = n.content || '(no content)';
  let contentHtml;
  if (n.lang === 'markdown') {
    contentHtml = `<div class="md-wrap">${marked.parse(txt)}</div>`;
    setTimeout(() => mermaid.run({ querySelector: '#panel .mermaid' }), 50);
  } else {
    const lang  = n.lang || 'plaintext';
    const hl    = hljs.highlight(txt, { language: lang, ignoreIllegals: true });
    const count = txt.split('\n').length;
    const start = n.start_line || 1;
    const nums  = Array.from({ length: count }, (_, i) => start + i).join('\n');
    contentHtml = `<div class="code-wrap"><div class="line-nums">${nums}</div><pre><code class="hljs language-${lang}">${hl.value}</code></pre></div>`;
  }
  return `
    <div class="ntitle">${esc(n.label)}</div>
    <div class="nlayer" style="word-break:break-all;font-size:10px">${esc(n.sym)}</div>
    <div class="nlayer">${esc(n.file)}</div>
    <div style="margin-top:6px">${badge}</div>
    ${contentHtml}`;
}

function renderNode(n) {
  if (!n) return '<div class="hint">(no content)</div>';
  const rel = n.abs_path ? relUrl(n.abs_path, n.start_line) : '';
  const url = rel ? `<a class="open-link" href="${rel}" target="_blank">Open file</a>` : '';
  const txt = n.content || '(no content)';
  let contentHtml;
  if (n.lang === 'markdown') {
    contentHtml = `<div class="md-wrap">${marked.parse(txt)}</div>`;
    setTimeout(() => mermaid.run({ querySelector: '#panel .mermaid' }), 50);
  } else {
    const lang  = n.lang || 'plaintext';
    const hl    = hljs.highlight(txt, { language: lang, ignoreIllegals: true });
    const count = txt.split('\n').length;
    const start = n.start_line || 1;
    const nums  = Array.from({ length: count }, (_, i) => start + i).join('\n');
    contentHtml = `<div class="code-wrap"><div class="line-nums">${nums}</div><pre><code class="hljs language-${lang}">${hl.value}</code></pre></div>`;
  }
  let sgHtml = '';
      const badge = stateBadge(sl.state, sl.ok);
      return `<div class="sg-item">${badge}<span class="sg-sym" title="${esc(sl.symbol)}">${esc(sl.symbol_short)}</span><span class="sg-file" title="${esc(sl.file)}">${esc(sl.file)}</span></div>`;
    }).join('');
    sgHtml = `<div class="sg-header">SUBGRAPH</div><div class="sg-list">${items}</div>`;
  }
  return `<div class="ntitle">${esc(n.label)}</div><div class="nlayer">${esc(n.layer)}</div>${url}${contentHtml}${sgHtml}`;
}

// ── Subgraph hover show/hide ──────────────────────────────────────────────────

([]).forEach(e => {
});

  const result = [];
  const queue  = [nodeId];
  while (queue.length) {
    const id = queue.shift();
  }
  return result;
}

let hideTimer  = null;
let shownForId = null;

  if (hideTimer) { clearTimeout(hideTimer); hideTimer = null; }
  if (shownForId === bilink_id) return;
  shownForId = bilink_id;
    cy.getElementById(id).style('display', 'element');
  });
    if (shown.has(e.data('source'))) e.style('display', 'element');
  });
}

}

    cy.getElementById(id).style('display', 'none');
  });
    if (shown.has(e.data('source'))) e.style('display', 'none');
  });
}

cy.on('mouseover', 'node[type="file"]', evt => {
  const n = evt.target.data();
});
cy.on('mouseout', 'node[type="file"]', evt => {
  const n = evt.target.data();
});

// ── Click handlers ────────────────────────────────────────────────────────────
cy.on('tap', 'node[type="file"]', evt => {
  const n = evt.target.data();
  document.getElementById('panel').innerHTML = renderNode(n);
});


cy.on('tap', 'edge', evt => {
  const e      = evt.target.data();
  const src    = cy.getElementById(e.source).data();
  const tgt    = cy.getElementById(e.target).data();
  const uuid   = e.uuid   || e.label || '';
  const states = e.states || '';
  const link0  = e.link0  || '';
  const link1  = e.link1  || '';
  document.getElementById('panel').innerHTML = `
    <div class="frag-label">link.0 — <code style="color:#a5d6ff;font-size:10px;word-break:break-all">${esc(link0)}</code></div>
    ${renderNode(src)}
    <div class="bilink-sep">
      <div class="bilink-sep-line"></div>
      <div class="bilink-sep-label">${esc(uuid)} · ${esc(states)}</div>
      <div class="bilink-sep-line"></div>
    </div>
    <div class="frag-label">link.1 — <code style="color:#a5d6ff;font-size:10px;word-break:break-all">${esc(link1)}</code></div>
    ${renderNode(tgt)}`;
  setTimeout(() => mermaid.run({ querySelector: '#panel .mermaid' }), 50);
});

function esc(s) {
  return (s||'').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

function relUrl(absPath, startLine) {
  const htmlDir = window.location.pathname.replace(/\/[^\/]*$/, '');
  const h = htmlDir.split('/').filter(Boolean);
  const f = absPath.split('/').filter(Boolean);
  let i = 0;
  while (i < h.length && i < f.length && h[i] === f[i]) i++;
  const rel = '../'.repeat(h.length - i) + f.slice(i).join('/');
  const frag = startLine > 1 ? '#L' + startLine : '';
  return rel + frag;
}
</script>
</body>
</html>"#;

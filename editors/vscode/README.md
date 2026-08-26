# Bilinker — extensión VS Code

Integración de [Bilinker](https://github.com/anibalanto/bilinker) y [Lattice](../../README.md) en VS Code. Provee hover contextual sobre bilinks via LSP y comandos para visualizar el grafo de dependencias entre capas.

## Requisitos

- **Rust / Cargo** — [rustup.rs](https://rustup.rs)
- **Node.js ≥ 18** — para compilar la extensión desde fuente
- **VS Code ≥ 1.85**

## Instalación

### 1. Instalar los binarios Rust

Desde la raíz del workspace (`crates/`):

```bash
# CLI principal
cargo install --path crates/bilinker-cli

# Language Server
cargo install --path crates/bilinker-lsp
```

Ambos binarios quedan en `~/.cargo/bin/`. Verificar que ese directorio esté en el `PATH`:

```bash
bilinker --version
bilinker-lsp --version
```

### 2. Instalar la extensión

**Opción A — desde el `.vsix` precompilado** (recomendado):

```bash
code --install-extension bilinker-0.1.0.vsix
```

En VS Code con Flatpak:

```bash
flatpak run com.visualstudio.code --install-extension bilinker-0.1.0.vsix
```

**Opción B — compilar desde fuente**:

```bash
cd editors/vscode
npm install
npm run bundle        # genera out/extension.js con esbuild (incluye vscode-languageclient)
npx @vscode/vsce package
```

`node_modules/` queda excluido del `.vsix` vía `.vscodeignore`; el bundle incluye todas las dependencias. Para abrir una ventana de desarrollo sin empaquetar, presionar `F5` en VS Code.

### 3. Recargar VS Code

Después de instalar la extensión, recargar la ventana (`Ctrl+Shift+P → Developer: Reload Window`). El servidor LSP arranca automáticamente al abrir cualquier archivo.

## Uso

### Hover

Al posicionar el cursor sobre un fragmento que tiene bilinks asociados, VS Code muestra en el tooltip el contenido del extremo opuesto formateado en Markdown: código con syntax highlighting, sección markdown con títulos y tablas.

### Code lens

Cada línea con bilinks muestra una lente `⬡ N bilink(s)`. Al hacer click se abre un panel lateral con los IDs de los bilinks del fragmento.

### Comandos

| Comando | Descripción |
|---------|-------------|
| `Bilinker: Open Graph for Current File` | Grafo HTML para el archivo o directorio activo |
| `Bilinker: Open System Graph` | Grafo HTML de todas las capas del workspace |

Acceder vía `Ctrl+Shift+P` → buscar `Bilinker`.

El grafo abre en un panel interno de VS Code con el visor interactivo (Cytoscape.js). Click en un nodo muestra el contenido del fragmento.

## Solución de problemas

**"bilinker-lsp binary not found"** — el LSP no está en el `PATH`. Opciones:

1. Agregar `~/.cargo/bin` al PATH del sistema.
2. O instalar via `cargo install --path crates/bilinker-lsp` como se indica arriba.

**El grafo aparece vacío** — verificar que exista al menos un archivo `.bilink/` en la capa actual y que los bilinks hayan sido aceptados (`bilinker accept .`).

**La extensión no activa** — revisar el Output panel (`Ctrl+Shift+U`) seleccionando `Bilinker` en el dropdown para ver los logs del LSP.

# El grafo

`lattice graph` recorre el grafo agregado desde un selector y emite los nodos y aristas alcanzados. Es el único comando de consulta: recorrer las cadenas de bilinks y subir por el call graph hasta encontrar bilinks son el mismo traversal con distinto conjunto de aristas habilitadas. Solo lectura: no escribe ningún archivo.

```
lattice graph [<selector>]
  [--up | --down | --both]
  [--via <kinds>]
  [--guarantee <nivel>]
  [--state <filtro>]
  [--depth <n>]
  [--cross]
  [--format <tree|flat|json|dot|html>]
  [--recursive]
```

| Flag | Descripción |
|---|---|
| `--up` | Sigue aristas dirigidas en sentido inverso (callers). |
| `--down` | Sigue aristas dirigidas en sentido directo (callees). |
| `--both` | Ambos sentidos. Default cuando no se especifica ninguno. |
| `--via <kinds>` | Lista de `kind` habilitados: `bilink,governs,issue,call,doclink,external`. Default: todos. |
| `--guarantee <nivel>` | Garantía mínima: `accepted`, `derived`, `asserted`. Default: `asserted`, o sea todas. |
| `--state <filtro>` | Solo aristas con ese estado en alguno de sus extremos. `non-ok` selecciona todo lo distinto de OK. |
| `--depth <n>` | Profundidad máxima. Default: sin límite, y 6 con `--cross`. |
| `--cross` | Recorrido de impacto: expande el call graph en cada nodo y cruza las aristas `accepted`. Ver "Las dos maneras de recorrer". |
| `--format` | `tree` por defecto. Ver "Las salidas". |
| `--recursive` | Recolecta también desde las capas descendientes. Se delega en el proveedor `bilink`. |

Los dos comandos que lattice reemplazó en bilinker son casos particulares:

```bash
# recorrer las cadenas de un archivo
lattice graph commands/check.md --via bilink

# subir por el call graph desde lo que tiene drift
lattice graph . --state non-ok --up --via bilink,call

# lo mismo, desde una posición
lattice graph src/check.rs:210:5 --up --via bilink,call
```

## El grafo compuesto

### El grafo lleva sus aristas y el estado de cada proveedor

El grafo se compone una vez por consulta con las aristas de los proveedores que enumeran ([provider.md](provider.md), "La composición"), indexadas por nodo incidente, y con el estado de todos los proveedores registrados. Los nodos son los extremos de las aristas: no hay nodos sueltos.

### Los filtros se aplican después de componer y antes de recorrer

`--via`, `--guarantee` y `--state` se aplican sobre las aristas compuestas, no al pedirlas: el estado de los proveedores refleja el grafo completo que se pudo ver. Y se aplican antes del traversal: definen por qué aristas se puede caminar, no solo cuáles se muestran.

## El selector

### Un selector resuelve a los nodos de partida del traversal

| Selector | Comportamiento |
|---|---|
| `.` o `*` | El grafo entero de la capa, sin traversal. Con `--recursive`, de todas las capas. |
| `<archivo>:<línea>:<col>` | Los nodos que cubren esa posición, del más específico al más general, más un nodo sintético de ancho cero en esa posición. |
| `<uuid>` | Los dos extremos de la arista cuyo `ref` empieza con ese uuid. |
| `<archivo>` | Todos los nodos cuyo path es ese archivo o termina con él. |

El nodo sintético de `<archivo>:<línea>:<col>` existe porque el LSP razona sobre posiciones, no sobre los rangos que declaró bilinker: preguntarle por el byte 0 de un archivo entero no significa nada. Ese nodo es su punto de entrada, y la contención lo conecta con lo que haya declarado encima. La capa del nodo es la del directorio de invocación relativa a la raíz más externa.

Una posición que ningún nodo cubre no resuelve, y el comando sale con 1.

## El recorrido

### Los proveedores que expanden bajo demanda aportan sus aristas antes de recorrer

Sobre cada nodo de partida se piden las aristas incidentes a los proveedores que implementan `edges_from`, y el grafo se recompone con ellas, deduplicado. Si no, el traversal solo vería lo que los que enumeran ya habían puesto. Con `--cross` se piden además en cada nodo que se alcanza.

### El traversal cruza de una garantía a otra por contención

```
BFS desde los nodos de partida:
  - en cada nodo alcanzado, sumar al frente los nodos relacionados por contención,
    en cualquier sentido
  - expandir por las aristas incidentes que la dirección permita
  - cortar por visited-set y por --depth
```

El salto por contención no consume profundidad: no es un paso del grafo, es reconocer que dos proveedores nombraron lo mismo. Es donde se cruza de una garantía a otra: al llegar a una función vía una arista `call`, la contención responde si hay un endpoint `accepted` que la contiene. Ese es el salto de "esto podría estar afectado" a "esto está roto".

`--depth n` deja pasar las aristas que salen de un nodo a distancia menor que `n`: con `--depth 1`, solo el primer salto.

### Las aristas no dirigidas se recorren en ambos sentidos siempre

`--up` y `--down` solo afectan a las dirigidas: hacia arriba se entra a una arista dirigida por su destino, hacia abajo por su origen. Con `--both` se pasa por cualquiera.

### Una rama que alcanza una arista `accepted` se detiene, salvo desde el nodo de partida

Sin ese corte, el traversal seguiría subiendo más allá del límite del subgrafo que alguien documentó, que es justamente el borde que interesa. La arista `accepted` se emite; lo que hay del otro lado, no. Con `--cross` no hay corte.

### Un nodo ya visitado no se expande

Visited-set sobre la forma canónica del nodo. Un ciclo termina sin volver a recorrerse.

## Las dos maneras de recorrer

### Sin `--cross`, el recorrido llega hasta el borde de lo documentado

Es la manera de contestar *"¿qué documentado alcanza esto?"*: qué spec gobierna una función, qué bilinks referencian un archivo, qué specs alcanza un cambio subiendo un salto por las llamadas. Expande el call graph sólo en los nodos de partida y para en la primera arista `accepted`, así que no necesita tope de profundidad.

### Con `--cross`, el recorrido cruza lo documentado y sigue del otro lado

Es la manera de contestar *"¿qué toca este cambio?"* cuando la respuesta está varios bilinks más allá: desde un servicio del back, los flujos funcionales que lo usan, pasando por el endpoint que lo llama, el bilink al servicio del front y el componente que llama a ese servicio.

Medido el 2026-09-16 sobre `sge`, con `jdtls` y `typescript-language-server` listos: `lattice graph DashboardServiceImpl.java:118:40 --up --cross` llega a los escenarios `TAB-J-09` y `TAB-U-05` de `tableros.feature` y a ningún otro flujo, con 8 aristas en 11,6 s. Sin `--cross`, el mismo recorrido para en el bilink del endpoint.

- **El call graph se expande en cada nodo que se alcanza**, no sólo en los de partida.
- **Una arista `accepted` se cruza, y el recorrido sigue del otro lado.**
- **Desde un nodo al que se llegó por una arista `accepted` no se toma otra.** Sería volver por la misma, o saltar a otro bilink del mismo fragmento. Las llamadas sí se siguen. El nodo se marca visitado junto con cómo se llegó a él: alcanzado después por una llamada, puede tomar sus aristas `accepted`.
- **La profundidad tiene tope**, 6 si no se pide otro con `--depth`: cada llamada y cada arista `accepted` cuentan un paso, y el salto por contención no cuenta.

## Las salidas

Todas muestran el `kind` y la garantía de cada arista: un consumidor tiene que poder distinguir de un vistazo qué parte del resultado es verificable y qué parte es inferencia de un language server.

### `tree` muestra cada arista con su `kind` y su garantía

```
$ lattice graph . --state non-ok --up

proveedores: bilink OK · lsp OK · doc OK

◆ fac79bf8  [ALTERED]
  .stratum/impl::crates/bilinker/src/check.rs#5100~7300
  ↕ bilink (accepted, commit ca76a590)
  .::commands/check.md#1240~2180

◆ 9662a432  [OK]
  .stratum/impl::crates/bilinker/src/check.rs#1200~2400
  ↑ call (derived)
  .stratum/impl::crates/bilinker/src/check.rs#5100~5100
```

La línea de proveedores va por stderr, y también `(sin aristas)` cuando no hay ninguna.

### `flat` es una línea por arista

`ref`, `kind`, garantía, estado o `-`, origen y destino, separados por tabulador. Para scripting.

### En `json`, `providers` va primero y siempre está presente

```json
{
  "providers": [
    {"name": "bilink", "status": "available"},
    {"name": "lsp",    "status": "unavailable", "reason": "daemon no responde"}
  ],
  "nodes": [
    {"id": ".::commands/check.md#1240~2180", "layer": ".", "path": "commands/check.md"}
  ],
  "edges": [
    {
      "from": ".stratum/impl::crates/bilinker/src/check.rs#5100~7300",
      "to":   ".::commands/check.md#1240~2180",
      "kind": "bilink", "guarantee": "accepted", "provider": "bilinker",
      "directed": false, "ref": "fac79bf8-...",
      "state": ["OK", "ALTERED"], "commit": ["ca76a590", "ca76a590"]
    }
  ]
}
```

Incluso cuando todos respondieron: un consumidor no debería tener que inferir la completitud del grafo a partir de su contenido.

### En `dot` la garantía se codifica en el trazo

```bash
lattice graph . --format dot | dot -Tsvg > graph.svg
```

Nodos agrupados en `subgraph cluster_N` por capa. Continuo para `accepted`, punteado para `derived`, punteado fino para `asserted`. Un grafo que mezcla lo verificado con lo inferido sin marcarlo induce a confiar en la inferencia.

### `html` es un archivo autocontenido con el grafo interactivo y el panel de detalle

```bash
lattice graph . --format html > graph.html
xdg-open graph.html
```

Sin servidor. Con:

- Grafo interactivo con Cytoscape.js: zoom, pan, clusters por capa en columnas por profundidad stratum (spec izquierda, impl derecha), file-groups que agrupan fragmentos del mismo archivo.
- Panel de detalle al hacer click en un nodo: el contenido del fragmento. `.md` renderizado; código con syntax highlighting y números de línea; link `file://` para abrirlo en el sistema.
- Panel de detalle al hacer click en una arista: los dos fragmentos vinculados, con el `ref` y el estado en el separador.
- Fragmentos distintos del mismo archivo como nodos separados.

La garantía viaja al visor: sin ella, una inferencia del LSP se ve igual que una referencia verificada.

Requiere conexión a internet para las CDN de Cytoscape y highlight.js.

## El código de salida

### El código de salida distingue el grafo degradado del completo

| Código | Condición |
|---|---|
| 0 | Traversal completado con todos los proveedores registrados disponibles y completos. |
| 1 | Ninguna arista alcanzada, o error de lectura o de configuración. |
| 3 | Traversal completado en modo degradado: algún proveedor no disponible o incompleto. |

El 3 permite que un pipeline de CI distinga "no hay impacto" de "no pude ver el impacto". Un proveedor `Unavailable` o `Degraded` reduce el grafo sin abortar la consulta, y cada uno deja su aviso por stderr:

```
warn: proveedor lsp no disponible (daemon no responde)
warn: proveedor lsp incompleto (daemon recién arrancado — el language server está indexando)
```

Con `--via call` como único `kind` y el proveedor caído, el resultado es vacío y el código de salida es 3: el degradado gana sobre el vacío.

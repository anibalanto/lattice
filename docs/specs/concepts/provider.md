# El proveedor

Un proveedor es una fuente de aristas. Lattice no produce ninguna: descubre proveedores, les pregunta por sus aristas en un scope, y compone el resultado. La contención sí la calcula lattice, y no es una arista.

## El contrato

### Todo proveedor declara sus `kinds()` con garantía fija antes de emitir

```
kinds()                 → [(kind, guarantee)]     qué emite y con qué garantía
available(scope)        → Availability            si puede responder ahora
edges(scope)            → [Edge]                  sus aristas en ese scope
edges_from(scope, node) → [Edge]                  aristas incidentes a un nodo
```

La garantía de un `kind` es fija y la declara el proveedor, no cada arista. Un proveedor puede emitir varios `kind`:

```
bilink    → [(bilink, accepted), (governs, accepted), (issue, accepted)]
lsp       → [(call, derived)]
doc       → [(doclink, asserted), (external, asserted)]
```

### `available()` se consulta antes de componer, no al primer fallo

```
Available                    puede responder, y lo que devuelve es completo
Degraded { reason }          responde, pero lo que devuelve está incompleto
Unavailable { reason }       no puede responder
```

Un proveedor que falla a mitad de un traversal deja un grafo incompleto que ya se reportó como completo.

`Degraded` no es un matiz cosmético: separa "puedo pedirle aristas" de "lo que devuelve alcanza para afirmar que no hay más". El caso que lo motiva es el daemon recién arrancado: responde al ping enseguida, pero el language server detrás sigue indexando, así que `callers` devuelve vacío. Reportarlo como `Available` haría pasar "todavía no sé" por "no hay llamadas", que es la confusión más cara que puede cometer este subsistema.

### Un proveedor que contesta incompleto lo dice junto con sus aristas

`available()` no siempre puede saber de antemano si lo que va a devolver está completo. Un proveedor que lo descubre al contestar entrega sus aristas junto con la razón, y queda `Degraded` con esa razón: las aristas se componen, y el grafo no se afirma completo.

### Un proveedor `Degraded` cuenta como grafo incompleto a los efectos del código de salida

A un proveedor `Degraded` se le piden aristas, igual que a uno `Available`. Lo que no se afirma es que el grafo esté completo: el código de salida lo dice.

### Un proveedor que expande bajo demanda implementa `edges_from`

Sin `edges_from`, lattice enumera todo el scope con `edges` y filtra. Con `edges_from`, el traversal expande de a un nodo, pidiéndoselas a los proveedores disponibles: necesario para el proveedor LSP, donde enumerar todas las llamadas del proyecto es inviable pero preguntar por los callers de una función es barato. Para los que enumeran, `edges_from` devuelve vacío: sus aristas ya salieron por `edges`.

## Los tres proveedores

### `bilink` se alimenta de `bilinker graph --format json`

Lee los bilinks de la capa y, con `--recursive`, de las capas descendientes. Emite una arista por cadena, entre los dos tips estructurales, nunca entre los nodos intermedios de la cadena.

Bilinker entrega los nodos en forma canónica, con los paths Stratum resueltos a la raíz de capa concreta, la topología de cadena resuelta y los tramos vigentes que el último `check` dejó en su cache, con la declaración de cada nodo de varios tramos en `declaration` ([node.md](node.md), "Un nodo con declaración contiene además lo que cae adentro de ella"). Nada de eso es conocimiento que lattice pueda tener sin duplicar el formato de bilinker: bilinker sabe qué significa un bilink y cómo resolverlo; lattice sabe cómo componer aristas heterogéneas.

Que `--recursive` se delegue en bilinker, en vez de que lattice recorra `.stratum/`, es la misma línea: dónde vive una capa es conocimiento de Stratum y del formato bilink, no del grafo agregado.

Está `Unavailable` cuando la capa no tiene `.bilink/` o cuando el ejecutable `bilinker` no se encuentra.

### Un `graph` que sale con 3 deja a `bilink` `Degraded`

`bilinker graph` sale con 3 cuando emite aristas y algún bilink se quedó afuera por no tener rango en la cache, como uno aceptado después del último `check`. El proveedor compone las aristas que salieron y queda `Degraded`, con lo que bilinker dijo por stderr como razón. Cualquier otra salida distinta de 0, como la de una capa sin ningún rango, es una falla, y el proveedor queda `Unavailable`.

### `state` y `commit` los recibe en la arista y no los calcula

Son los dos campos que ningún otro proveedor emite y que impact necesita: `state`, para filtrar por no-OK sin abrir archivos; `commit`, como baseline de `git log <commit>..HEAD`. Los dos son derivados del lado de bilinker: `state` vive en su cache y `commit` se re-deriva de git si falta. Sin ellos en la arista, un consumidor tendría que reabrir los bilinks para completarlos, que es exactamente la duplicación que lattice elimina.

### `lsp` implementa `edges_from`, no `edges`: el call graph se expande

Consulta a `lspd` por `callers` y `callees`. No enumera: listar todas las llamadas de un proyecto es inviable, y el call graph no se persiste. Se expande nodo por nodo durante el traversal, y `edges` devuelve vacío a propósito.

Requiere resolver el anclaje del nodo antes de preguntar ([node.md](node.md), "El anclaje"). Cada llamada que responde el daemon se convierte a forma canónica: un rango de ancho cero en el byte donde arranca la línea, en la capa cuyo ancestro tiene `.bilink/`. El LSP no dice hasta dónde llega la función, y un punto alcanza para que la contención lo ubique dentro de lo que se haya declarado encima.

### `lsp` pregunta a la puerta de su workspace

El socket es el de su workspace, que se deriva de él: quien pregunta pasa la raíz que le va a preguntar. Con una puerta por sistema, dos proyectos abiertos se pisaban el daemon y el segundo recibía una negación en vez de "no sé".

Es el único proveedor cuya ausencia es esperable en operación normal. Si el daemon no responde, lo arranca ([daemon.md](daemon.md), "El auto-start sí es de lattice") y queda `Degraded` mientras el language server indexa. Que no se pueda arrancar es `Unavailable`, con la razón.

### `doc` extrae links de documentos markdown

Recorre los `.md` del scope, sin entrar en `node_modules`, `target`, `.git`, `.bilink` ni `out`. Emite `doclink` para destinos dentro del proyecto y `external` para URIs (`http://`, `https://`, `mailto:`). Es la única fuente de aristas `asserted`: nadie las verificó. Bilinker conecta specs con código y el LSP conecta código con código; esto conecta documentos entre sí.

Un `doclink` cuyo destino no existe se emite igual, con `broken`. Un link muerto en un documento es información, no un error de lattice. El ancla `#seccion` no forma parte del destino en el filesystem, y un link que es solo ancla no se emite.

Siempre está `Available`.

### El nodo de origen y el de destino son archivos completos, no fragmentos

Un link markdown apunta a un documento. Que un nodo de archivo completo contenga a los fragmentos de ese archivo ([node.md](node.md)) es lo que permite que un doclink alcance los bilinks declarados sobre sus partes. El archivo se nombra contra la capa cuyo ancestro más profundo tiene `.bilink/`, y contra la raíz más externa si ninguno lo tiene.

### Los links dentro de bloques de código y las imágenes no se emiten

Dos construcciones tienen forma de link y no lo son:

| Construcción | Por qué no |
|---|---|
| Links dentro de bloques de código | Un ejemplo no es una referencia. |
| Imágenes `![alt](x.png)` | Un embed no es una referencia a otro documento. Modelarlo pediría un `kind` propio, y hoy no hay consumidor que lo justifique. |

Un documento con estos cuatro casos produce una arista, no cinco:

````markdown
Una referencia real: [node](node.md).          ← se emite

```markdown
Ver [capture.md](capture.md) para el formato.   ← documentar la sintaxis
```

```markdown
Evaluá el cambio contra [la decisión]({{adr}}). ← plantilla de skill
```

```markdown
Ver el hilo en [impact](../threads/3a.md).      ← ejemplo del formato .task
```
````

El caso de la plantilla es el que decide la regla. `{{adr}}` no existe ni puede existir, así que aparecería como `broken` de forma permanente, y encontrar links muertos es el uso principal de este proveedor. Un falso positivo que nunca se puede resolver enseña a ignorar el resultado, que es la peor falla posible para una herramienta de verificación.

El de documentar la sintaxis es el más probable: es lo que pasa cuando una spec muestra un ejemplo del formato que define. Ahí el destino sí existe, así que no sale roto; el grafo simplemente afirma una relación que nadie escribió.

## La composición

### Todo resultado lleva el estado de cada proveedor registrado

```
1. Consultar available() de cada proveedor registrado.
2. Pedir edges(scope) a los disponibles.
3. Deduplicar.
4. Emitir las aristas junto con el estado de cada proveedor, disponible o no.
```

Un proveedor cuyo `edges` falla queda `Unavailable` con el error como razón, y uno que contesta incompleto queda `Degraded` con la suya. Un consumidor no debería tener que inferir la completitud del grafo a partir de su contenido.

La expansión con `edges_from` viene después, durante el traversal, sobre los nodos de partida ([graph.md](graph.md)).

### Un proveedor no disponible reduce el grafo y se reporta; nunca produce un resultado silenciosamente incompleto

```
warn: proveedor lsp no disponible (daemon no responde)
```

Un análisis sobre un grafo degradado sigue siendo válido; lo que no es válido es que el consumidor no pueda distinguirlo de uno completo.

### Los tres proveedores se registran siempre

`bilink`, `lsp` y `doc` participan de toda consulta, en ese orden. No hay configuración que los agregue ni los saque: un proveedor que no puede responder aparece como `Unavailable`, con su razón, y no desaparece del resultado.

### Un proveedor nunca escribe en sus fuentes al responder una query

Lattice es solo lectura. Lo poco que queda en disco, el endpoint y el pid del daemon, es de `lspd` y no de acá.

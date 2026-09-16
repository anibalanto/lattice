# El nodo

Un nodo es un fragmento direccionable del proyecto. Todos los proveedores emiten el mismo tipo de nodo: un fragmento de bilink, una función vista por el LSP y un documento markdown son el mismo tipo de cosa, vista por fuentes distintas.

## La forma canónica

### Todo nodo tiene forma canónica

```
<layer-root>::<path>#<start>~<end>[,<start>~<end>…]    fragmento (uno o más tramos de bytes)
<layer-root>::<path>                                    archivo completo
issue:<id>                                              ítem del tracker
<uri>                                                   recurso externo
```

`<layer-root>` es la ruta de la capa relativa a la raíz del proyecto (`.` para la capa raíz, `.stratum/impl`, …). `<path>` es relativo a la raíz de esa capa. Los offsets son bytes absolutos dentro del archivo, con la misma semántica que el rango de un capture de bilinker. Un fragmento de varias partes, como un capture `spring-controller`, lleva un tramo por parte, en orden de archivo: `#1847~1876,2114~2140,2149~2192,2218~2676`. El texto entre dos tramos no es del fragmento.

No hay nodos anónimos ni identificados por objeto.

### La forma canónica la produce el proveedor

Un proveedor que emite una arista hacia otra capa entrega el nodo ya resuelto: bilinker resuelve la cadena a través de capas antes de emitir, porque la topología de cadena es conocimiento de su formato.

### La identidad es igualdad exacta de forma canónica

Dos nodos son el mismo si su forma canónica coincide exactamente.

No se intenta unificar nodos con rangos parecidos. Un proveedor que dice `crates/bilinker/src/check.rs#5100~7300` y otro que dice `crates/bilinker/src/check.rs#5140~7300` producen dos nodos, y la relación entre ellos se expresa por contención, no por identidad. Intentar fusionarlos requeriría un criterio de tolerancia arbitrario que fallaría de formas silenciosas.

## La contención

Es la operación central del subsistema, porque es la que permite cruzar de una garantía a otra. El LSP dice "la función `check_structural` en `check.rs:210` llama a esto"; bilinker dice "tengo un endpoint aceptado que cubre los bytes 5100~7300 de `check.rs`". Sin contención son dos grafos disjuntos; con contención, un cambio en una función se conecta con la spec que la gobierna.

### La contención la calcula lattice, nunca un proveedor

```
contiene(a, b)  ⟺  a.layer == b.layer  ∧  a.path == b.path
                    ∧  ∀ tb ∈ b.tramos  ∃ ta ∈ a.tramos:  ta.start ≤ tb.start  ∧  tb.end ≤ ta.end
```

Lattice la calcula a partir de los tramos que recibe; ningún proveedor la emite, y no es una arista. Se calcula sobre cada tramo, no sobre el rango del primero al último: un punto entre dos tramos no está contenido. Todos los endpoints de un controller empiezan en la misma anotación de la clase, así que el rango del primero al último los anidaría a todos.

### Un nodo con declaración contiene además lo que cae adentro de ella

```
contiene(a, b)  ⟸  a.layer == b.layer  ∧  a.path == b.path
                    ∧  ∀ tb ∈ b.tramos:  a.declaración.start ≤ tb.start  ∧  tb.end ≤ a.declaración.end
```

La declaración es un tramo que el proveedor manda junto al nodo: el del nodo que declara lo que el fragmento nombra. Bilinker la manda para un fragmento de varias partes, y en un `spring-controller` es el método entero, con su cuerpo. No es parte de la identidad del nodo, que sigue siendo su forma canónica.

Es lo que decide qué contiene a una llamada que cae en el cuerpo de un método cuya captura es sólo la firma: la contiene el bilink de ese método, por su declaración, y ningún otro. Lo mismo el punto con que el LSP nombra al método, al comienzo de la línea de su nombre, que cae entre dos partes porque el nombre no se captura. La anotación de la clase, que comparten todos los endpoints del archivo, queda fuera de cada declaración, y un punto entre ella y el método no está contenido.

### La contención se define sobre bytes, no sobre líneas

Un consumidor que parte de una posición del LSP, que trabaja en líneas y columnas, convierte a byte antes de consultar. La conversión correcta es posición → byte, no rango → línea: comparar contra el byte inicial de la línea da falsos positivos cuando dos fragmentos comparten línea, y falsos negativos con rangos que empiezan a mitad de línea.

Un nodo cubre una posición cuando alguno de sus tramos o su declaración la contiene, con el fin exclusivo: `#10~20` cubre el byte 19 y no el 20.

### `cubriendo(pos)` devuelve los nodos ordenados de más específico a más general

```
cubriendo(<layer>::<path>#<pos>)  →  nodos cuyo rango contiene esa posición,
                                      del más específico al más general
```

El orden importa: si dos bilinks cubren la misma posición, el consumidor casi siempre quiere el más ajustado. Lo que se mide es el largo de lo que cubre la posición: el tramo que la contiene o, si ninguno, la declaración.

## Los nodos sin rango

### Un archivo completo contiene a todos los fragmentos de ese archivo

Un nodo de archivo completo no tiene rango y contiene a todo fragmento de ese archivo, y a ningún otro nodo. Es lo que permite que un link markdown, que apunta al archivo, alcance los bilinks declarados sobre sus partes. Un fragmento no contiene al archivo, y un archivo no se contiene a sí mismo.

### Los nodos `issue:<id>` y las URIs no participan de contención

No son fragmentos de un archivo de una capa: no tienen capa, ni path, ni rango. Están en el grafo por las aristas que los alcanzan, y nada más.

## El anclaje

Las dos fuentes localizan las cosas de manera incompatible: bilinker por rango de bytes, derivado de una query tree-sitter; el LSP por línea y columna del identificador. Para preguntarle al LSP por los callers de un nodo que vino de un bilink, hay que encontrar la posición del identificador dentro del rango.

### El anclaje de un nodo es la línea de su primer byte y la columna del identificador declarado

La línea es exacta: se cuenta desde el inicio del archivo hasta el primer byte del rango. El archivo se resuelve contra la raíz más externa del ecosistema, no contra el directorio de invocación: si no, un nodo de la capa de specs no se resolvería al correr desde el impl. La columna se mide desde el inicio de esa línea, no desde el byte del fragmento.

### La columna del identificador es la que sigue a la palabra que declara

Se busca la palabra clave de declaración (`fn`, `def`, `function`, `class`, `struct`, `trait`, `interface`, `enum`, `impl`, `type`, `void`) y se devuelve el token que la sigue. Mirar el primer token que no sea keyword no alcanza: `pub(crate) fn foo` daría `crate`, y la lista de modificadores no tiene fin. Sin palabra clave, como en una firma Java sin tipo, queda el primer token de la línea.

Es una heurística, y las aristas que produce son `derived`: fallar significa no encontrar una llamada, nunca afirmar algo falso.

### Un nodo sin anclaje resoluble no produce aristas `derived`, pero sigue en el grafo

Un nodo que no es fragmento, que no tiene rango o cuyo archivo no se puede leer no le pregunta nada al LSP. Se queda en el grafo con las aristas que ya tenía, y el traversal sigue.

# La arista

Una arista es una conexión entre dos [nodos](node.md) del grafo que declara siempre de dónde viene y qué garantiza. Es la unidad fundamental de lattice: todo el resto del modelo existe para que las aristas puedan convivir sin perder esas dos propiedades.

## La anatomía

### Toda arista tiene `provider` y `guarantee`

| Campo | Descripción |
|---|---|
| `from` / `to` | Nodos en forma canónica, ya resueltos entre capas. |
| `kind` | Tipo de conector. Ver "Tipos de arista". |
| `guarantee` | `accepted` · `derived` · `asserted`. Ver "La garantía". |
| `provider` | Quién emitió la arista. |
| `directed` | Si el orden `from → to` tiene significado semántico. |
| `ref` | Identificador de la arista en su fuente: UUID del bilink, símbolo LSP, path + anchor. |
| `state` | Estado que reporta el proveedor. Solo para `accepted`; ausente en el resto. |
| `commit` | Commit en que se aceptó cada extremo. Solo para `accepted`. |
| `broken` | El destino no se pudo resolver. Ausente cuando es falso. |

No existe una arista anónima.

### Solo las aristas `accepted` tienen `state` y `commit`

`state` es lo que reporta el dueño de una arista `accepted` sobre su propia aceptación, y lattice lo transporta sin interpretarlo: la semántica de un `ALTERED` es de bilinker.

`commit` existe porque es el baseline de todo diff: un consumidor que quiera saber qué cambió desde el último estado aceptado corre `git log <commit>..HEAD`. Sin él en la arista, tendría que reabrir el archivo del proveedor para recuperarlo.

`broken` no viaja en `state` a propósito. Un link muerto en un documento es otra cosa: una arista que existe y apunta a la nada.

## La garantía

El eje que distingue a las aristas no es su forma sino cuánto se puede afirmar a partir de ellas.

### Un consumidor puede filtrar por garantía, pero nunca recibe una arista sin ella

| Garantía | Significado | Qué afirma |
|---|---|---|
| `accepted` | Declarada por un humano y verificada por su dueño (hash + commit). | "Esto está vinculado, y alguien aceptó explícitamente el estado en que estaba." |
| `derived` | Calculada por una herramienta a partir del contenido actual. | "Esto probablemente está relacionado, según lo que una herramienta externa pudo inferir." |
| `asserted` | Escrita en el contenido, sin ninguna verificación. | "Alguien escribió que esto se relaciona." |

La distinción no es cosmética. Una arista `derived` proviene de un language server, que falla sistemáticamente con dispatch dinámico, trait objects, macros, callbacks e inyección de dependencias: su ausencia no prueba nada. Una arista `asserted` puede apuntar a un archivo que ya no existe. Una arista `accepted` es la única sobre la que se puede afirmar que hubo drift, porque es la única con un estado anterior aceptado contra el cual comparar.

Las garantías se ordenan por fuerza: `accepted` > `derived` > `asserted`.

### La garantía de un `kind` es fija y la declara el proveedor, no la arista

| `kind` | Proveedor | `guarantee` | Dirigida | `ref` |
|---|---|---|---|---|
| `bilink` | bilinker | `accepted` | no | `<uuid>.<N>` |
| `governs` | bilinker | `accepted` | no | `<uuid>` |
| `issue` | bilinker | `accepted` | no | `<uuid>` + id del ítem |
| `call` | proveedor LSP | `derived` | sí (caller → callee) | símbolo LSP |
| `doclink` | proveedor markdown | `asserted` | sí | path + anchor |
| `external` | proveedor markdown | `asserted` | sí | URI |

Un proveedor puede emitir varios `kind`, pero la garantía de cada `kind` es fija: no existe un `call` aceptado ni un `bilink` derivado.

`governs` todavía no lo emite nadie. Exige el endpoint de tipo bilink de bilinker, que está especificado y no implementado. El `kind` queda declarado acá porque lattice ya sabe transportarlo; lo que falta es el proveedor.

## La deduplicación

Dos proveedores pueden emitir la misma conexión.

### Se deduplica por `(from, to, kind)` y se conserva la garantía más fuerte

De dos aristas con la misma clave queda la de garantía más fuerte, con el proveedor que la emitió. Que la declarada gane a la derivada es deliberado: la declarada tiene un estado aceptado detrás.

Un corolario útil: si una conexión que un proveedor deriva automáticamente se declara además a mano, la declaración manual es un duplicado permanente que hay que mantener sincronizado. Declarar a mano solo se justifica para aristas que ningún proveedor puede derivar: llamadas entre lenguajes, entre repos, o hacia un binario externo.

### Los extremos se ordenan para comparar solo si la arista no es dirigida

En una dirigida el sentido es parte del hecho: dos documentos que se referencian mutuamente son dos links, no uno, y "a llama a b" no es "b llama a a".

## La frescura

### Lattice no escribe en ninguna fuente ni persiste el grafo

Cada consulta compone el grafo desde los proveedores:

- Las `accepted` las persiste su dueño; lattice lee también el `state` que ese dueño reporta.
- Las `derived` se calculan en el momento y no se escriben nunca.
- Las `asserted` se validan al leerlas, preguntando si el destino existe, y no se escriben nunca.

Persistir aristas derivadas ya se intentó en el ecosistema y se revirtió: un índice derivado bajo control de versiones se desincroniza del código y reporta drift que no existe.

### Lattice refleja lo que los bilinks dicen en el momento de la consulta

No corre `check` ni lo dispara. Un `state` desactualizado es un `check` que no se corrió, no un error de lattice: un consumidor que necesite estados frescos corre `bilinker check` antes de consultar. La aceptación de un cambio sigue siendo `bilinker accept`, invocado por una persona.

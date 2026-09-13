# El editor

La extensión de VS Code integra el ecosistema en el editor con tres capacidades: hover sobre fragmentos bilinkeados, code lens por línea y el visor de grafo. Vive en `editors/vscode/`.

## Dónde vive

### La extensión vive en lattice, y `bilinker-lsp` en bilinker

La extensión invoca dos binarios: `bilinker-lsp` para hover y code lens, y `lattice` para el visor. Ubicarla del lado de lattice no invierte ninguna dependencia, porque lattice ya consume a bilinker por CLI, y evita que el consumidor del CLI de lattice viva enterrado en el repo de bilinker.

`bilinker-lsp` se queda en bilinker: servir el otro extremo de un bilink al pasar el mouse es conocimiento de su formato.

No es un subsistema propio: no modela nada del dominio, es un cliente de dos herramientas que sí lo hacen.

## La activación

### La extensión activa al terminar el arranque y busca `bilinker-lsp`

Activa en `onStartupFinished`. Si no encuentra `bilinker-lsp`, muestra un error y no registra los comandos. Si lo encuentra, arranca el cliente LSP sobre stdio para todo archivo, porque un bilink puede referenciar cualquiera, sincroniza los cambios bajo `.bilink/`, y registra los tres comandos: `bilinker.openGraph`, `bilinker.openSystemGraph` y `bilinker.showBilinks`.

### El binario se busca en PATH y después en `~/.cargo/bin`

`findBinary(name)` intenta `which <name>`; si falla, porque el PATH es reducido como en Flatpak, verifica `~/.cargo/bin/<name>`. Vale para los dos binarios.

## Hover y code lens

### El hover y el code lens los sirve `bilinker-lsp`

Cuando el cursor está sobre un fragmento con bilinks, el tooltip muestra el contenido del extremo opuesto formateado en Markdown: código con syntax highlighting, sección markdown con títulos y tablas. Cada línea con bilinks muestra una lente `⬡ N bilink(s)`. Qué muestran es de bilinker; la extensión solo aloja el cliente.

### Al hacer click en la lente se abre un panel con los ids de los bilinks del fragmento

`bilinker.showBilinks` recibe el archivo y los ids, y abre un WebviewPanel lateral que los lista.

## Los comandos de grafo

Corren `lattice graph`, que compone el grafo de todos los proveedores y lo renderiza. Requieren el ejecutable `lattice`, además de `bilinker-lsp`; si no está, lo dicen con un error.

### `bilinker.openGraph` corre `lattice graph <ruta-relativa> --format html` desde la raíz del workspace

Es el grafo del archivo actual. La ruta es relativa a la raíz del workspace, y el resultado se muestra en un WebviewPanel lateral con `enableScripts: true`, con el nombre del workspace como título.

### `bilinker.openSystemGraph` corre `lattice graph . --recursive --format html` desde la raíz del workspace

Es el grafo del sistema completo, con el mismo panel.

### El código de salida 3 muestra el grafo y avisa

`lattice` devuelve 3 cuando algún proveedor no respondió, típicamente el daemon LSP apagado. El grafo es válido igual, así que la extensión lo muestra y avisa con un warning, con la última línea del stderr, en vez de tratarlo como error. Cualquier otro código distinto de 0 es un error, con el stderr en el mensaje. Confundir "grafo incompleto" con "falló" dejaría al usuario sin visor cada vez que el daemon no esté corriendo, que es el caso normal.

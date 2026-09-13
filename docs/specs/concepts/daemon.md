# El daemon

Los language servers que alimentan al proveedor `lsp` los mantiene vivo `lspd`, que no es de lattice: es una capa aparte, con su propio ciclo de vida, y bilinker también le pregunta. Lattice lo consume como cualquier otro, por el socket, con el cliente compartido `lspd-client`.

## El comando

### `lattice daemon` no es un comando propio: es el mismo, reexportado

```
lattice daemon start  [--workspace <path>]
lattice daemon stop   [--workspace <path>]
lattice daemon status [--workspace <path>]
```

`lattice daemon start` hace exactamente lo que `lspd start`, y existe porque el daemon salió de lattice y quitarle a la gente el comando que venía usando sería cobrarle a ella una reorganización que no pidió. Los tres delegan. Qué hacen, qué imprimen y con qué código salen está en la spec de `lspd`, y no se repite acá: dos specs del mismo comando divergen el día que alguien toca una.

| Código | Condición |
|---|---|
| 0 | Éxito |
| 1 | Daemon ya corriendo (`start`), no corriendo (`stop`, `status`), o error al arrancarlo |

### El daemon es el de un workspace, no el del sistema

La ruta del socket se deriva del workspace, así que parar o consultar "el" daemon dejó de tener sentido cuando hay uno por proyecto. Los tres subcomandos toman `--workspace`, y por defecto es el directorio actual. Cuando no hay daemon, el mensaje dice en qué endpoint no lo hay: que no haya uno acá no dice nada de los otros.

## Quién lo arranca

### El auto-start sí es de lattice

El proveedor `lsp` intenta conectarse antes de cada consulta. Si no hay nadie, arranca el daemon del workspace y espera a que responda.

| Resultado | Estado del proveedor |
|---|---|
| El daemon ya estaba corriendo | `Available` |
| Se arrancó recién y respondió | `Degraded`: el language server está indexando |
| No se pudo arrancar | `Unavailable`, con la razón |

La distinción importa: el daemon contesta al `ping` apenas arranca, pero rust-analyzer y sus pares tardan bastante más en indexar. Durante esa ventana `callers` devuelve vacío, y reportar `Available` haría pasar "todavía no sé" por "no hay llamadas".

Es una política de lattice y no del daemon, y por eso vive de este lado. `lspd` no decide cuándo arrancar; bilinker, el otro consumidor, tomó la decisión contraria: no lo arranca nunca y degrada a "no verificado". Las dos son legítimas y ninguna es del daemon. El mecanismo, dónde está el binario y cuánto se espera, lo pone `lspd-client`.

### Ningún comando de lattice requiere el daemon

Su ausencia siempre degrada, nunca aborta.

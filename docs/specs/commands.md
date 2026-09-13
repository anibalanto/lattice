# Los comandos

`lattice` expone una consulta y el ciclo de vida del daemon, y nada más. La consulta es [el grafo](concepts/graph.md); el daemon es de `lspd`, y acá solo se lo reexporta ([daemon.md](concepts/daemon.md)).

| Comando | Uso | Qué hace |
|---|---|---|
| `graph` | `graph [<selector>] [--up \| --down \| --both] [--via <kinds>] [--guarantee <nivel>] [--state <filtro>] [--depth <n>] [--format <tree\|flat\|json\|dot\|html>] [--recursive]` | Recorre el grafo agregado desde un selector y emite los nodos y aristas alcanzados, con el estado de cada proveedor. Solo lectura. Sale con 3 si algún proveedor no respondió ([graph.md](concepts/graph.md)). |
| `daemon start` | `daemon start [--workspace <path>]` | Arranca el `lspd` del workspace, el de acá por defecto. Si ya hay uno, lo dice y retorna 1 ([daemon.md](concepts/daemon.md)). |
| `daemon stop` | `daemon stop [--workspace <path>]` | Manda `shutdown` al `lspd` del workspace. Si no hay, lo dice y retorna 1 ([daemon.md](concepts/daemon.md)). |
| `daemon status` | `daemon status [--workspace <path>]` | Imprime el pid y el endpoint del `lspd` del workspace, y sus language servers con su estado y cuántas preguntas contestó cada uno ([daemon.md](concepts/daemon.md)). |

El selector de `graph` es opcional: sin él es `.`, el grafo entero de la capa actual.

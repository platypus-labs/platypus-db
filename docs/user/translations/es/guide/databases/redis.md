<!-- source: 8b43e38d9f7d -->
[← Bases de datos](README.md)

# Redis

El driver está construido sobre el crate `redis` (RESP3 cuando el servidor lo habla, con rustls para
TLS). Abre Redis 6 y posteriores, y Redis Stack para las claves JSON. El editor usa **comandos de
Redis**, uno por línea. El driver es parte de la app; no se descarga nada.

## Conectar

| Pegas | Se interpreta como |
|---|---|
| `redis://:s%40cret@cache:6380/3` | TCP sin cifrar, solo contraseña, base de datos 3 |
| `redis://app:pw@cache/0` | un usuario de ACL y su contraseña |
| `rediss://app:pw@cache` | TLS (verificado: `verify-full`) |

El puerto por defecto es 6379. La ruta (o el campo Base de datos) es el **número de base de datos**,
`0` si está vacío; cualquier otra cosa es un error. El usuario y la contraseña son los de la ACL: una
contraseña sola inicia sesión como el usuario por defecto.

**TLS** está activado para las URL `rediss://` y para los modos SSL de `require` en adelante
(`redis://` significa sin TLS y `prefer` no lo activa, porque Redis recibe TLS en un puerto propio).
`require` no comprueba el certificado; `verify-ca` y `verify-full` sí. Un archivo de CA y un
certificado de cliente van en los campos de TLS. La sesión abre una conexión por cada base de datos
que usa, porque un `SELECT` en una conexión compartida movería a todos los demás que la usan.

## Privilegios

Las reglas de ACL del inicio de sesión deciden lo que pueden hacer el navegador y los comandos. El
navegador lee `INFO` y, para cada tipo de clave, ejecuta `SCAN … TYPE`; navegar por un tipo ejecuta
`BROWSE`, nunca `KEYS`. No hay pestañas de administración.

## Qué funciona

- Comandos en el editor, entrecomillados como los entrecomilla `redis-cli`; las líneas con `#` y `--`
  son comentarios. Las respuestas son filas, con las respuestas anidadas como JSON en el árbol del
  documento (`F7`).
- `SELECT n` mueve la sesión a otra base de datos. `MULTI` pone los comandos en cola hasta `EXEC`, que
  los envía como una sola transacción y muestra cada respuesta.
- El navegador lista las bases de datos que contienen claves, cada una con una tabla por tipo de
  clave: `string`, `hash`, `list`, `set`, `zset`, `stream`, y `json` con RedisJSON. Abrir una ejecuta
  `BROWSE db0.hash`, opcionalmente con `MATCH` y `LIMIT`, leyendo las claves grandes por trozos.
- **La cuadrícula edita filas**: valores, TTL, nombres de clave, campos de hash, miembros y
  puntuaciones; las filas nuevas añaden claves y elementos, y las filas borradas los quitan. La vista
  previa muestra los comandos, que se ejecutan como un solo `MULTI`/`EXEC`; un comando que el servidor
  rechaza no deshace los demás. Un *Script de deshacer* devuelve los valores. Una clave, un campo o un
  miembro que ya existe detiene el guardado antes de ejecutar nada.
- `FLUSHDB` y `FLUSHALL` preguntan antes, como un `UPDATE` sin `WHERE`; en las conexiones de
  producción, los comandos que cambian el servidor (`CONFIG SET`, `SHUTDOWN`) preguntan como lo hace
  el DDL. Las conexiones de solo lectura solo ejecutan comandos que leen.
- Cancelar detiene un comando bloqueante (`BLPOP`, `XREAD BLOCK`).
- El modo terminal ejecuta `redis-cli` si está en el PATH, con la contraseña en su entorno.
- Todos los comandos y tipos de clave se describen en [FEATURES.md](../../FEATURES.md#redis).

## Qué no funciona

- Las suscripciones (`SUBSCRIBE` y similares) y `MONITOR` se rechazan: se apoderarían de la conexión.
- Sin confirmación manual: `MULTI`/`EXEC` es la única transacción.
- Sin explain, sin DDL de objetos, sin herramientas SQL (editor de tablas, diagrama ER, comparación
  de esquemas, constructor de consultas, importación SQL), sin instantáneas antes de cambios.

## Agentes

`run_query` y `sample_rows` (`BROWSE … LIMIT n`) funcionan; cualquier comando que no sea de lectura se
pregunta antes. Los comandos que muestran las credenciales del servidor (`ACL GETUSER`, `ACL LIST`, y
un `CONFIG GET` cuyo patrón coincide con un ajuste de contraseña) se rechazan a los agentes de solo
lectura. Rechazados en Redis: `explain`, `get_er_diagram`, `propose_edits` y
`query_across_databases`. Consulta [Agentes y la CLI](../../AGENTS.md).

## Límites conocidos

- Anterior a la 6.0: conecta con un aviso de que algunas funciones pueden no funcionar.
- El recuento de claves de una tabla solo se muestra cuando la base de datos tiene hasta 10.000 claves.
- Los valores que no son UTF-8 se muestran entrecomillados con secuencias de escape.

## Probado en

La batería de pruebas de conformidad se ejecuta en Redis Stack 7.4 (con RedisJSON): metadatos,
`BROWSE` en cada tipo, comandos y sus errores, guardar y deshacer ediciones de la cuadrícula,
cancelar comandos bloqueantes, usuarios de ACL y selección de base de datos, y una conexión perdida
que se reabre. Redis 6 es la versión más baja que acepta el driver, pero la batería no la ejecuta.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

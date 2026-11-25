<!-- source: b5b424fa73d7 -->
[← Bases de datos](README.md)

# ClickHouse

El driver habla con la **interfaz HTTP** de ClickHouse y lee los resultados en el formato de stream de
Arrow. Es parte de la app; no se descarga nada. Navega, consulta y diseña tablas, pero no edita filas
en la cuadrícula.

## Conectar

| Pegas | Se interpreta como |
|---|---|
| `clickhouse://default@ch.example.com/logs` | HTTP en el puerto 8123, base de datos `logs` |
| `clickhouse://ch.example.com/logs?sslmode=require` | HTTPS; sin puerto en la URL, 8443 |
| `jdbc:clickhouse://host:8123/db` | una URL JDBC |
| `https://host:8443` en un formulario de ClickHouse | la URL HTTP(S) del servidor |

Usa el puerto **HTTP**, no el nativo (9000). El usuario es `default` si está vacío; la contraseña se
envía en la cabecera `X-ClickHouse-Key`. El campo Base de datos ejecuta `USE <database>` después de
conectar.

**TLS** se elige con el modo, ya que HTTP no puede negociarlo: `disable` y `prefer` (el valor por
defecto) conectan en **HTTP sin cifrar**; `require` y los modos `verify` usan HTTPS. Con HTTPS el
puerto por defecto es 8443 cuando la URL no da ninguno; si rellenas el formulario, escribe tú el
puerto. `require` no comprueba el certificado, `verify-ca` comprueba la cadena, `verify-full`
también el nombre del host; `sslrootcert` nombra el archivo de tu CA, y si no se usan los certificados
de confianza del sistema.

Una conexión es una sesión HTTP: `SET`, `USE` y las tablas temporales duran entre sentencias.

## Privilegios

Navegar lee `system.databases`, `system.tables`, `system.columns` y `system.data_skipping_indices`; el
inicio de sesión ve lo que ClickHouse le deja ver allí. Las pestañas leen:

- **Parámetros**: `system.settings` y `system.server_settings`.
- **Panel**: `system.metrics`, `system.events`, `system.parts` (tamaños de las bases de datos) y
  `system.replicas`; lo que el inicio de sesión no puede leer muestra "—".
- **Usuarios y roles**: `system.users`, `system.roles`, `system.grants` y `system.role_grants`.
  También se listan los usuarios definidos en los archivos de configuración del servidor; el servidor
  se niega a cambiarlos.

## Qué funciona

- Consultas, resultados, el navegador, el autocompletado, el DDL de los objetos, la búsqueda de
  metadatos y el diseñador de tablas (incluida la cláusula de motor y los índices de salto).
- Explain muestra el plan de `EXPLAIN indexes = 1`; no hay Explain analyze.
- Cancelar detiene la sentencia en el servidor con `KILL QUERY`.
- Pestañas Parámetros, Panel y Usuarios y roles. Sin pestañas de sesiones, bloqueos, sentencias ni
  tablas e índices.
- Exportaciones, e importaciones a ClickHouse como sentencias `INSERT`.

## Qué no funciona

- **Sin edición de datos en la cuadrícula**: dice "ClickHouse tables are not edited in place".
  Guardar comprobaría el recuento de filas de cada cambio dentro de una transacción, y ClickHouse no
  tiene ni transacciones ni recuentos de filas para las actualizaciones. Cambia las filas con SQL.
- **Sin transacciones**: la barra de confirmación manual está oculta; cada sentencia surte efecto
  por sí sola.
- Sin claves foráneas ni restricciones de unicidad (la clave primaria solo ordena los datos).

## Agentes

Un agente de solo lectura, y una conexión de solo lectura, envían cada sentencia salvo `SET` con
`readonly = 1`, así que el servidor rechaza las escrituras y las funciones de tabla que llegan a otros
servidores o a sus archivos (`url`, `s3`, `remote`, `file` y similares). La sentencia de un agente
también se comprueba contra una lista de funciones de tabla que solo leen datos locales. Consulta
[Agentes y la CLI](../../../../AGENTS.md). El modo terminal no está disponible: `clickhouse-client` usa el
puerto nativo.

## Límites conocidos

- HTTP sin cifrar salvo que el modo sea `require` o más estricto: el `prefer` por defecto no cifra.
- Solo se usa la interfaz HTTP; no se puede llegar a un servidor que solo expone el puerto nativo.

## Probado en

La batería de pruebas de conformidad se ejecuta en ClickHouse 26.3. Las versiones anteriores de
ClickHouse no están probadas, y la app no tiene versión mínima para ella.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

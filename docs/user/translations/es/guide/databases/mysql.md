<!-- source: 6f25d7181e09 -->
[← Bases de datos](README.md)

# MySQL y MariaDB

Un solo driver (el cliente `mysql_async`) abre MySQL 5.7 y posteriores y MariaDB 10.6 y posteriores.
Los dos tienen su icono en la cuadrícula de bases de datos; el driver detecta a qué servidor llegó y
lo muestra en la conexión (`Conectado: MariaDB 11.4`). No se descarga nada.

## Conectar

| Pegas | Se interpreta como |
|---|---|
| `mysql://user:pass@host:3306/db?sslMode=REQUIRED` (o `mariadb://`) | la forma URL; puerto por defecto 3306 |
| `jdbc:mysql://host:3307/db`, `jdbc:mariadb://…` | una URL JDBC |
| `Server=db;Port=3306;Database=shop;Uid=app;Pwd=…` | una cadena de conexión de Connector/NET, si además lleva `Driver=` o la pegaste en un formulario de MySQL |

Parámetros de URL que la app conoce: `sslmode` (también escrito `ssl-mode`, `sslMode`, `ssl_mode`),
`sslrootcert`, `sslcert`, `sslkey`, `application_name` y `connect_timeout`.

**TLS**: `disable` lo desactiva. `prefer` (el valor por defecto) usa TLS y vuelve a texto plano
cuando el servidor no tiene o solo ofrece versiones que el cliente rechaza (las imágenes de MySQL 5.7
lo hacen); nunca vuelve atrás al iniciar sesión con un token. `prefer` y `require` no comprueban el
certificado. `verify-ca` comprueba la cadena contra `sslrootcert` y no el nombre del host;
`verify-full` comprueba ambos. Se entienden los nombres propios de MySQL (`REQUIRED`,
`VERIFY_IDENTITY`). Un certificado de cliente necesita `sslcert` y `sslkey`.

**El inicio de sesión** es con contraseña, o con un token de AWS IAM, Google Cloud IAM o Microsoft
Entra ID creado por la herramienta `aws`, `gcloud` o `az`; el token viaja por TLS como pide el
servidor. Kerberos (el `auth_gssapi` de MariaDB, el plugin de Kerberos de MySQL) aún no está
soportado.

La conexión es siempre TCP: no se usa un socket Unix ni una tubería con nombre de Windows.

## Privilegios

Navegar necesita lo que el inicio de sesión puede ver. Las pestañas de administración leen:

- **Sesiones**: la lista de procesos, de `performance_schema.threads` en MySQL 8 y de
  `information_schema.PROCESSLIST` en MariaDB, MySQL 5.7 o un servidor con `performance_schema`
  desactivado. Cuánto de las sesiones de otros usuarios muestra el servidor es regla del servidor, que
  la app no comprueba.
- **Bloqueos**: `performance_schema.data_lock_waits`, `data_locks` y `metadata_locks` en MySQL 8 (ver
  las esperas de otras sesiones necesita `PROCESS`; sin él la lista no tiene esperas y lo dice),
  `INNODB_LOCK_WAITS` en `information_schema` en MariaDB y en MySQL más antiguo.
- **Sentencias más costosas**: `performance_schema.events_statements_summary_by_digest`. Si
  `performance_schema` está desactivado, la pestaña dice que hay que iniciar el servidor con él.
- **Tablas e índices**: `information_schema`, más `performance_schema` para saber cuánto se lee cada
  índice; sin él, el uso de los índices se desconoce.
- **Cancelar la sentencia** y **Terminar la sesión** ejecutan `KILL QUERY` y `KILL CONNECTION`.
- **Parámetros** y **Panel**: variables del servidor y contadores de estado.
- **Usuarios y roles**: las cuentas y los permisos del servidor.
- **Mantenimiento**: `ANALYZE`, `OPTIMIZE` y `CHECK TABLE`.
- **Copia de seguridad… y Restaurar…** ejecutan `mysqldump` y `mysql`, que deben estar en el PATH.

## Qué funciona

- Edición de datos en la cuadrícula, por clave primaria (o una clave única en columnas no nulas, o una
  clave virtual).
- Confirmación manual y rollback, y cancelar una sentencia en el servidor.
- Explain: MySQL 8.0.16 y posteriores dan `EXPLAIN FORMAT=TREE`; MySQL más antiguo y MariaDB dan
  `EXPLAIN FORMAT=JSON`. **Explain analyze** es `EXPLAIN ANALYZE` en MySQL 8.0.18 y posteriores y
  `ANALYZE FORMAT=JSON` en MariaDB; MySQL 5.7 y de 8.0.0 a 8.0.17 lo rechazan.
- Todas las pestañas de administración de arriba, y las herramientas de esquema (editor de tablas,
  diagrama ER, comparación).
- Las importaciones usan `LOAD DATA LOCAL` cuando el servidor lo permite (`local_infile` activado) y,
  si no, sentencias `INSERT`.
- Los errores nombran el servidor del que vienen, p. ej. `Unknown column 'x' in 'field list' (MySQL 5.7.44)`.

## Agentes

Los agentes de solo lectura y las conexiones de solo lectura ejecutan
`SET SESSION TRANSACTION READ ONLY`, y sus consultas se ejecutan en una transacción de solo lectura.
`server_activity` y `top_queries` funcionan. Una `@variable`, una tabla temporal o un `SET` de MySQL o
MariaDB hechos en una llamada de un agente siguen ahí en la siguiente. Consulta
[Agentes y la CLI](../../../../AGENTS.md).

## Límites conocidos

- Anterior a MySQL 5.7 o MariaDB 10.6: conecta, con un aviso de que algunas funciones pueden no
  funcionar.
- El editor de tablas guarda con el DDL de MySQL que confirma cada sentencia: un script fallido puede
  dejar parte aplicada.
- El modo terminal necesita `mysql` en el PATH.

## Probado en

La batería de pruebas de conformidad se ejecuta en MySQL 5.7, 8.0 y 8.4 y MariaDB 10.6 y 11.4, con un
certificado de cliente sobre TLS mutuo en MySQL 8.4, y con sesiones cerradas por el servidor.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

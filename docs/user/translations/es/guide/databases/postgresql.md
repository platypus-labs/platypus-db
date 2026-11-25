<!-- source: a2b0bfe9784d -->
[← Bases de datos](README.md)

# PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB y el resto

Un solo driver, construido sobre `tokio-postgres`, habla el protocolo de PostgreSQL. Abre PostgreSQL
12 y posteriores, y los servidores y servicios que hablan el mismo protocolo. No se descarga nada: el
driver es parte de la app.

## Conectar

Elige **PostgreSQL** en la cuadrícula de bases de datos, o pega una URL: el formulario se rellena
solo.

| Pegas | Se interpreta como |
|---|---|
| `postgres://user:pass@host:5432/db?sslmode=require` (o `postgresql://`) | la forma URL; el puerto por defecto es 5432 |
| `jdbc:postgresql://host/db?sslmode=require` | una URL JDBC, como las guardan DBeaver y DataGrip |
| `host=db port=5432 dbname=shop user=app sslmode=require` | una cadena de conexión de libpq |
| `Host=db;Database=shop;Username=app;Password=…` | una cadena de conexión de Npgsql |

Parámetros de URL que la app conoce: `sslmode`, `sslrootcert`, `sslcert`, `sslkey`, `application_name`,
`connect_timeout` y `pgbouncer=true`. Cualquier otro parámetro se omite, y el formulario dice cuál.
Una lista de hosts de failover (`h1:5432,h2:5432`) se queda con el primer host.

**TLS** sigue el `sslmode` de libpq:

| Modo | Qué ocurre |
|---|---|
| `disable` | sin TLS |
| `prefer` (el valor por defecto) | TLS si el servidor lo ofrece, y si no texto plano; no se comprueba el certificado |
| `require` | TLS, certificado sin comprobar |
| `verify-ca` | TLS, se comprueba la cadena del certificado, no el nombre del host |
| `verify-full` | TLS, se comprueban la cadena y el nombre del host |

La cadena se comprueba contra el archivo de `sslrootcert` si lo hay, y si no contra los certificados
de confianza del sistema. Un certificado de cliente (`sslcert` y `sslkey`) se envía en todos los
modos que usan TLS.

**El inicio de sesión** es con contraseña, o con un token que la herramienta de línea de comandos de
una nube crea en cada conexión: AWS IAM (`aws`), Google Cloud IAM (`gcloud`) y Microsoft Entra ID
(`az`) están en la opción *Autenticación* de la conexión. La herramienta debe estar instalada y con la
sesión iniciada en tu equipo. O con **Kerberos**, más abajo.

**Kerberos (GSSAPI).** Elige *Ticket de Kerberos (GSSAPI)* en *Password from*. El campo de contraseña
desaparece: no se pide, no se guarda y no se envía ninguna contraseña. Si el servidor pide una, o te
deja entrar sin pedir Kerberos, la conexión se rechaza y lo dice.

- **El nombre del servicio** es `postgres` salvo que lo rellenes (el `krb_srvname` del servidor). El
  ticket que se pide es `service/host`, en tu realm por defecto: el host debe ser el nombre del
  principal del servicio, no una dirección IP ni un alias, porque la app no lo resuelve. A través de
  un túnel SSH o un reenvío de puerto sigue siendo el nombre del servidor.
- **El usuario** es el rol de PostgreSQL, que el servidor asigna a partir de tu principal
  (`include_realm` y `pg_ident.conf`). Si se deja vacío, la app usa el nombre de usuario de tu
  sistema.
- **El ticket** en Linux viene de `kinit`, en la caché que nombra `KRB5CCNAME` (o la de por defecto),
  o de un keytab: con `KRB5_CLIENT_KTNAME` definida, Kerberos obtiene el ticket por sí mismo. Esas
  variables deben estar definidas donde se inicia la app. En Windows es el ticket de tu sesión de
  Windows.
- **Sin ticket** la prueba dice qué ejecutar: `kinit`, las dos variables y `klist`, seguidos del
  propio texto de Kerberos. Lo mismo vale para un ticket caducado, un servicio desconocido y un KDC
  inaccesible.
- **`psql`, `pg_dump` y `pg_restore`** inician sesión de la misma manera (`PGKRBSRVNAME`,
  `PGREQUIREAUTH=gss`, y sin `PGPASSWORD`). Su copia de libpq debe haberse compilado con GSSAPI.
- **Qué compilaciones lo tienen.** Los paquetes de Windows (SSPI) y de Linux (GSSAPI, MIT Kerberos).
  Los de macOS no: ningún Mac lo ha compilado todavía, y el formulario lo dice. La feature de Cargo
  `kerberos` de `platypus-driver-pg` lo activa; sin ella no se enlaza ni se ejecuta nada de Kerberos.
- **El cifrado GSS** (`hostgssenc`, `gssencmode`) no se usa: usa TLS para cifrar, con líneas `hostssl`
  o `host`. Un `krbsrvname` en una URL pegada se omite, como cualquier otro parámetro que la app no
  conoce.

**Poolers.** Detrás de PgBouncer, Supavisor o el pooler de Neon en transaction mode, elige *Transaction
mode* en el formulario (o añade `pgbouncer=true` a la URL). Las sentencias se ejecutan entonces sin
nombre, porque la siguiente transacción puede estar en otra sesión del servidor. El formulario lo
sugiere para los hosts que terminan en `.pooler.supabase.com` y para los hosts de Neon con `-pooler`
en el nombre. En ese modo el depurador de PL/pgSQL está desactivado, y las sesiones de solo lectura de
los agentes dependen solo de la comprobación de la sentencia.

Los otros servidores tienen iconos propios, que solo fijan valores por defecto:

| Icono | Valores por defecto que fija |
|---|---|
| CockroachDB | puerto 26257 |
| YugabyteDB | puerto 5433, usuario y base de datos `yugabyte` |
| TimescaleDB | ninguno |
| Supabase | usuario y base de datos `postgres`, TLS `require` |
| Neon, AlloyDB | TLS `require` |

## Privilegios

Navegar necesita solo lo que el servidor muestra al inicio de sesión. Las pestañas de administración
leen esto:

- **Sesiones** y **Bloqueos**: `pg_stat_activity` y `pg_locks`. La app lista las sesiones de cliente
  que devuelve el servidor; cuánto de las sesiones y sentencias de otros usuarios muestra el servidor
  al inicio de sesión es regla del servidor, que la app no comprueba. *Cancelar la sentencia* y
  *Terminar la sesión* llaman a `pg_cancel_backend` y `pg_terminate_backend`; el servidor decide si
  el inicio de sesión puede.
- **Sentencias más costosas**: la extensión `pg_stat_statements`. Sin ella la pestaña dice cómo
  activarla (añadirla a `shared_preload_libraries`, reiniciar, `CREATE EXTENSION`).
- **Tablas e índices**: `pg_stat_user_tables` y `pg_stat_user_indexes`.
- **Parámetros** y **Panel**: `pg_settings`, `pg_stat_database`, `pg_stat_activity` y las vistas de
  replicación; los standbys solo se listan a un miembro de `pg_monitor`. Una cifra que el inicio de
  sesión no puede leer muestra "—" y por qué.
- **Usuarios y roles**: `pg_roles` y `pg_auth_members`; cambiarlos necesita los permisos que
  necesitan las sentencias.
- **Mantenimiento**: `VACUUM`, `ANALYZE` y `REINDEX` sobre la tabla.
- **Copia de seguridad… y Restaurar…** ejecutan `pg_dump`, `pg_restore` y `psql`, que deben estar en
  el PATH.
- **Depurar…** necesita la extensión `pldebugger` en el servidor.

## Qué funciona

- Edición de datos en la cuadrícula, por clave primaria (o una clave única en columnas no nulas, o una
  clave virtual que tú defines). Los arrays y los valores compuestos se editan un elemento o campo por
  línea.
- Transacciones con confirmación manual, y sentencias canceladas en el servidor.
- Explain y Explain analyze; el plan se lee del JSON o del texto de PostgreSQL.
- Varias bases de datos por conexión, particiones, geometrías PostGIS, arrays y tipos compuestos.
- Las importaciones y copias a PostgreSQL usan `COPY`.
- Todas las pestañas de administración de arriba, el depurador de PL/pgSQL y las herramientas de
  esquema (editor de tablas, diagrama ER, comparación).

**CockroachDB y YugabyteDB** se abren con el mismo driver, pero la app detecta de qué servidor se
trata, y las herramientas de servidor que dependen de las vistas propias de PostgreSQL se desactivan:
sin pestañas de sesiones, bloqueos, panel, sentencias, tablas e índices, parámetros, usuarios ni
mantenimiento, y sin depurador. CockroachDB tiene su propio formato de plan (Explain y Explain
analyze funcionan) y las importaciones van como sentencias `INSERT` en lugar de `COPY`. Una tabla de
CockroachDB sin clave se edita por su `rowid` oculto. TimescaleDB (probado), Supabase, Neon y AlloyDB
(no probados contra los servicios) son PostgreSQL para la app, con todo lo anterior; lo que un servicio
alojado restringe para tu rol es regla suya.

## Agentes

Los agentes de solo lectura y las conexiones de solo lectura abren la sesión con
`default_transaction_read_only` (PostgreSQL, CockroachDB y YugabyteDB), así que el propio servidor
rechaza las escrituras; detrás de un pooler en transaction mode solo cuenta la comprobación de la
sentencia. `server_activity` y `top_queries` funcionan en PostgreSQL propiamente dicho, no en
CockroachDB ni en YugabyteDB. Consulta [Agentes y la CLI](../../AGENTS.md).

## Límites conocidos

- Anterior a la 12: la app conecta y avisa de que algunas funciones pueden no funcionar.
- El depurador está desactivado detrás de un pooler en transaction mode.
- El modo terminal necesita `psql` en el PATH.

## Probado en

La batería de pruebas de conformidad se ejecuta en PostgreSQL 12, 14, 16 y 17, TimescaleDB 2.30 en
PostgreSQL 17, CockroachDB 26.2 y YugabyteDB 2025.2, además de PgBouncer en transaction mode, un
certificado de cliente sobre TLS mutuo, y el depurador en un servidor con `pldebugger`. Supabase,
Neon y AlloyDB no están probados: son el driver de PostgreSQL con valores por defecto.

Kerberos está probado con un ticket real de un KDC real (MIT Kerberos, en el perfil `kerberos` de
`docker/compose.yml`) en PostgreSQL 17, desde un cliente Linux con GSSAPI: un ticket de `kinit`, un
keytab, TLS, `psql`, sin ticket, un ticket caducado y un nombre de servicio erróneo. **El inicio de
sesión mediante SSPI de Windows no se ha probado con un ticket real:** un equipo Windows fuera de un
dominio no puede llegar al KDC de pruebas sin cambiar ajustes del sistema (`ksetup`). En Windows solo
se prueba lo que ocurre sin ticket. Tampoco se ha probado una línea `sspi` del servidor (PostgreSQL en
Windows).

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

<!-- source: 3830a0c639a7 -->
[← Bases de datos](README.md)

# SQL Server y Azure SQL

El driver habla TDS por sí mismo (un `tiberius` con parches), así que no hay nada que instalar ni
descargar. Está escrito para SQL Server 2017 (14.0) y posteriores, y para Azure SQL (no probado contra
el servicio: consulta el final de esta página).

## Conectar

| Pegas | Se interpreta como |
|---|---|
| `sqlserver://sa:pass@host:1433/master?encrypt=true&trustServerCertificate=true` (o `mssql://`) | la forma URL; puerto por defecto 1433 |
| `jdbc:sqlserver://host:1433;databaseName=shop;encrypt=true` | una URL JDBC (el driver de Microsoft, jTDS o el antiguo `jdbc:microsoft:`) |
| `Server=tcp:db.example.com,1433;Database=shop;User Id=sa;Password=…;Encrypt=True` | una cadena de conexión ADO.NET u ODBC |

En una URL, `database` o `databaseName` nombra la base de datos, y `encrypt` y
`trustServerCertificate` se leen como las leen las cadenas JDBC y ADO.NET (salvo que también se dé
`sslmode`).

**Las instancias con nombre** (`host\SQLEXPRESS`) no se buscan mediante SQL Server Browser: indica el
puerto de la instancia. Una cadena pegada con una instancia y sin puerto lo dice.

**TLS**: `disable` desactiva el cifrado. `prefer` (el valor por defecto) cifra y no comprueba el
certificado (los servidores suelen tener uno autofirmado), y vuelve a texto plano si el servidor no
tiene cifrado. `require` cifra sin comprobar. `verify-ca` y `verify-full` comprueban ambos la cadena
**y** el nombre del host, porque la biblioteca TLS siempre comprueba el nombre; apunta `sslrootcert` al
archivo de tu CA, y si no se usan los certificados de confianza del sistema.

**Inicio de sesión**:

- *Inicio de sesión de SQL Server*: usuario y contraseña.
- *Inicio de sesión de Windows* (`Integrated Security=SSPI` en una cadena pegada): solo cuando
  PlatypusDB se ejecuta en Windows; en otros sistemas es un error.
- *Microsoft Entra ID*: un token creado por la herramienta `az`, que debe estar instalada y con la
  sesión iniciada. Así se llega al inicio de sesión de Entra de Azure SQL.

**Azure SQL**: la pasarela puede enviar al cliente al nodo propio de la base de datos; la app sigue una
de esas redirecciones, salvo a través de un túnel SSH, que lleva a una sola dirección.

## Privilegios

Navegar necesita lo que el inicio de sesión puede ver. Las pestañas de administración leen:

- **Sesiones** y **Bloqueos**: `sys.dm_exec_sessions`, `dm_exec_requests`, `dm_exec_connections` y
  `dm_tran_locks`. Para ver lo que hacen otros inicios de sesión, el inicio de sesión necesita
  `VIEW SERVER STATE`.
- **Sentencias más costosas**: `sys.dm_exec_query_stats`, la caché de planes.
- **Tablas e índices**: `sys.tables`, `dm_db_partition_stats`, `dm_db_index_usage_stats` y
  `sys.stats`.
- **Parámetros**: `sys.configurations`; *Abrir en el editor* escribe una sentencia `sp_configure`.
- **Panel**: contadores de rendimiento, tamaños de `sys.databases` y `sys.master_files`, y grupos de
  disponibilidad donde los haya; una cifra que el inicio de sesión no puede leer muestra "—".
- **Usuarios y roles**: los inicios de sesión, roles y permisos del servidor.
- **Mantenimiento**: `UPDATE STATISTICS`, `ALTER INDEX ALL … REBUILD` y `DBCC CHECKTABLE`.

## Qué funciona

- Edición de datos en la cuadrícula, por clave primaria (o una clave única en columnas no nulas, o una
  clave virtual).
- Confirmación manual y rollback; cancelar una sentencia en ejecución (una attention de TDS) conserva
  la sesión, su transacción y sus tablas temporales.
- Varios conjuntos de resultados de un mismo batch, y mensajes `PRINT`.
- Varias bases de datos por conexión: una conexión que nombra su base de datos muestra solo esa;
  *Mostrar todas las bases de datos* lista las demás.
- Explain muestra el plan **estimado** (`SHOWPLAN`). Explain analyze se rechaza.
- *Terminar la sesión* es `KILL`. Cancelar la sentencia de otra sesión no es posible en SQL Server,
  así que la pestaña solo ofrece terminar la sesión.
- Las importaciones usan la copia masiva de SQL Server (inserts con `OPENJSON` para las tablas con
  columnas `money`, `xml` o heredadas).
- Herramientas de esquema (editor de tablas, diagrama ER, comparación), también para columnas
  calculadas y con collation, índices avanzados y tablas temporales. Los índices XML y espaciales se
  dejan fuera de una comparación.
- El modo terminal ejecuta `sqlcmd`, si está en el PATH.

## Agentes

En SQL Server, solo la comprobación de la sentencia impide las escrituras a los agentes de solo
lectura (sin sesión de solo lectura en el servidor); la transacción de un agente se revierte.
`server_activity` y `top_queries` funcionan. Consulta [Agentes y la CLI](../../../../AGENTS.md).

## Límites conocidos

- Anterior a 2017: la app conecta y avisa de que algunas funciones pueden no funcionar.
- Sin Explain analyze y sin búsqueda de instancias con nombre.

## Probado en

La batería de pruebas de conformidad se ejecuta en SQL Server 2017 y 2022 (contenedores), y el driver
ODBC genérico se prueba contra el mismo contenedor 2022. Azure SQL no está probado: sus reglas de
redirección, inicio de sesión de Entra y cifrado están en el código pero no se han ejecutado contra el
servicio.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

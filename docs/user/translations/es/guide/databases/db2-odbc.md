<!-- source: 1206da228de0 -->
[← Bases de datos](README.md)

# IBM Db2 y ODBC (Windows)

A Db2, y a cualquier base de datos con un driver ODBC o un origen de datos (DSN), se llega mediante el
administrador de drivers ODBC del sistema. Por ahora esto funciona **solo en Windows**: en macOS y
Linux una conexión de Db2 u ODBC dice que a los drivers ODBC solo se llega en Windows.

## Primer uso: el driver ODBC de IBM (Db2)

Cuando eliges **IBM Db2** para una conexión nueva y no hay ningún driver ODBC de IBM registrado en
este equipo, se abre una ventana sobre el formulario: *IBM Db2 requiere IBM Data Server Driver for
ODBC and CLI*. Muestra el paquete (versión 12.1.4), su tamaño, los términos de IBM y de dónde viene,
y comprueba la descarga con un SHA-256. **Descargar e instalar** lo descarga en la carpeta de datos
de la app y lo registra con `db2cli install -setup`; para ello **Windows pide permiso de
administrador (Control de cuentas de usuario)**. **Cancelar** deja el formulario utilizable, y la
oferta vuelve en *Probar la conexión* y en la primera apertura.

Un driver que ya está en el sistema cuenta: la app busca uno llamado `IBM DB2 ODBC DRIVER…` o
`IBM DATA SERVER…`, ya lo haya creado su propio registro o un cliente completo de IBM, y toma el más
reciente por nombre. La opción `driver` de la conexión sustituye esa elección. *Ajustes › Drivers*
lista y elimina el driver descargado, y muestra los drivers ODBC y los orígenes de datos del sistema.

## Conectar

**IBM Db2**: host, puerto (50000 por defecto), **base de datos** (se requieren tanto el host como la
base de datos), usuario y contraseña. La app construye ella misma la cadena de conexión (`DATABASE`,
`HOSTNAME`, `PORT`, `PROTOCOL=TCPIP`, `UID`, `PWD`). TLS: los modos `disable` y `prefer` conectan sin
él; `require` y superiores añaden `Security=SSL`, y `sslrootcert` pasa a ser `SSLServerCertificate`.
Los servidores Db2 escuchan TLS en un puerto distinto del de texto plano, así que define el puerto. A
través de un túnel SSH se usa el extremo local del túnel.

**Plataforma** elige el tipo de servidor Db2:

- *Db2 for Linux, UNIX and Windows (LUW)*, el valor por defecto, también para servidores 8.x antiguos.
- *Db2 for z/OS*: el mismo driver de IBM, puerto 446 por defecto, y la **base de datos** es el nombre
  de la ubicación. El driver de IBM llega a z/OS solo con una licencia de Db2 Connect en este equipo.
- *Db2 for IBM i* (AS/400): el propio *IBM i Access ODBC Driver* de IBM (IBM i Access Client Solutions,
  que se instala desde IBM, no con la app; el driver más antiguo iSeries Access también sirve). El
  host es el nombre del sistema, la base de datos es opcional y fija la biblioteca por defecto, no hay
  puerto, y TLS añade `SSL=1`. No pasa por un túnel SSH, porque el driver abre varios puertos del
  servidor.

**Otro driver ODBC** (la opción genérica): indica una de estas

- un **origen de datos (DSN)** que el sistema conozca, escrito o tomado de las sugerencias;
- un nombre de **driver** (sugerido a partir de los drivers ODBC del sistema) con una **cadena de
  conexión** que contenga el resto, como `Server=host;Database=sales`;
- una **cadena de conexión** sola, usada tal como está escrita (puede nombrar su propio `DRIVER=`).

Los campos de usuario y contraseña se añaden como `UID` y `PWD` salvo que la cadena ya los tenga.
Una cadena ADO.NET u ODBC pegada con `Driver={…}` se lee en el formulario correspondiente cuando el
driver es SQL Server, PostgreSQL, MySQL/MariaDB o ClickHouse; cualquier otro driver o un `DSN=` pasa a
ser una conexión ODBC genérica con la cadena tal cual.

## Privilegios

Lo que tenga el inicio de sesión de la base de datos. El navegador usa las llamadas de catálogo de
ODBC (`SQLTables`, `SQLColumns`, `SQLPrimaryKeys`, `SQLForeignKeys`). No hay pestañas de
administración.

## Qué funciona

- Consultas y resultados, con los tipos que informa el driver.
- Confirmación manual y rollback.
- Cancelar: la llamada de cancelación de ODBC no es accesible, así que una sentencia que aún no ha
  empezado se abandona junto con su sesión (la app abre una nueva); una que está devolviendo filas se
  detiene cuando se descarta su stream.
- El navegador, a partir de las llamadas de catálogo. El SQL de Db2 se escribe con las comillas de Db2
  (los nombres sin comillas pasan a mayúsculas, las filas se limitan con `FETCH FIRST`); el driver
  genérico escribe SQL estándar.

## Qué no funciona

- Sin explain, sin DDL de objetos, sin pestañas de administración, sin carga masiva (las
  importaciones usan `INSERT`).
- **Los valores de texto y binarios de más de 64 KiB son un error**, en lugar de un buffer enorme.
- La **edición de datos en la cuadrícula** no está probada en estas bases de datos. La cuadrícula no
  la bloquea, pero sin savepoints un guardado fallido en modo de confirmación manual revierte toda la
  transacción de la sesión y lo dice.
- El modo terminal no está disponible.

## Agentes

Los agentes de solo lectura dependen solo de la comprobación de la sentencia. Consulta
[Agentes y la CLI](../../../../AGENTS.md).

## Límites conocidos

- Solo Windows.
- Db2 no tiene contenedor en el entorno de pruebas, así que la cadena de conexión de Db2 y la
  detección del driver solo se comprueban con pruebas unitarias.

## Probado en

El driver ODBC genérico se ejecuta contra SQL Server 2022 mediante el propio driver `SQL Server` de
Windows: tipos, resultados largos, errores con su SQLSTATE, metadatos, confirmar y revertir, cancelar
una sentencia en espera, los literales y tipos propios de SQL Server, e importaciones. IBM Db2 no
está probado contra un servidor.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

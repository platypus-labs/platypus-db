<!-- source: 6fd0f54f48e3 -->
[← Bases de datos](README.md)

# Snowflake, BigQuery, Databricks, Trino y otros drivers ADBC

A estas bases de datos se llega mediante drivers **ADBC** (Arrow Database Connectivity), bibliotecas
que la app carga al conectar. Snowflake, BigQuery, Databricks y Trino tienen un icono cada una;
cualquier otra base de datos con un driver ADBC conecta mediante **Otro driver ADBC**.

## Primer uso: la descarga del driver

Las cuatro bases de datos nombradas necesitan su driver en este equipo. Cuando eliges una para una
conexión nueva y el driver no está instalado, se abre una ventana sobre el formulario: *Snowflake
requiere un driver* (y así con las demás). Dice que es un driver ADBC, que se descarga una vez para
todas las conexiones de esa base de datos, y muestra el paquete, su versión, su licencia y de dónde
viene: el [registro de drivers dbc](https://docs.columnar.tech/dbc/) (Columnar). **Descargar** lo
obtiene, comprueba su SHA-256 (un paquete sin él se rechaza) y lo guarda con los datos de la app;
**Cancelar** deja el formulario utilizable, y la oferta vuelve en *Probar la conexión* y en la
primera apertura.

La oferta solo lee lo que hay en este equipo, así que aparece también sin conexión; la descarga
necesita red. *Ajustes › Drivers* lista los drivers instalados y los elimina, busca versiones más
recientes una vez al día (o con *Buscar actualizaciones*), instala otra versión o vuelve a una
anterior con *Versiones…*, y puede instalar las actualizaciones automáticamente. Tras un cambio, se
prueban las conexiones guardadas que usan el driver (conectar, versión, esquemas, `SELECT 1`), con
*Go back* si una falla. La opción de otro driver no descarga nada: tú indicas un archivo de
biblioteca.

## Conectar

Cada base de datos tiene sus propios campos:

| Base de datos | Campos |
|---|---|
| Snowflake | **Cuenta** (por ejemplo `xy12345.eu-west-1`), usuario, contraseña, warehouse, rol, base de datos, esquema |
| BigQuery | **Proyecto**, dataset, y un archivo de clave de cuenta de servicio (JSON); vacío significa las Application Default Credentials de Google. Sin contraseña |
| Databricks | **Nombre de host del servidor**, **Ruta HTTP**, catálogo, esquema; el campo de contraseña es el **token de acceso** |
| Trino | **Host**, puerto (8080 por defecto), **usuario**, catálogo, esquema, contraseña (opcional) y TLS |
| Otro driver ADBC | el archivo de la **biblioteca del driver**, un símbolo de **punto de entrada** opcional, una URI, usuario y contraseña; cualquier otra opción que añadas se pasa al driver |

**Las opciones de TLS de Trino** son *Automático — HTTPS en el puerto 443, si no HTTP*,
*Desactivado — HTTP*, *Activado — HTTPS, verifica el certificado* y *Activado — HTTPS, sin verificar*.

**Otro driver ADBC**: la biblioteca es el archivo del driver que descargaste o compilaste. El punto de
entrada solo hace falta para una biblioteca que no exporta `AdbcDriverInit` (para la biblioteca ADBC
de DuckDB es `duckdb_adbc_init`). La URI y las opciones son lo que documente ese driver. Un archivo
que falta, o que no es un driver, se notifica al conectar.

## Privilegios

Lo que tenga la cuenta o el token en el servicio. El navegador lee los catálogos, esquemas, tablas y
columnas con la llamada de metadatos de ADBC.

## Qué funciona

- Consultas, resultados, el navegador y el autocompletado. El resultado llega en streaming como lotes
  de Arrow.
- Cancelar: la app pide al driver que cancele; los drivers escritos en Go solo cancelan sus propias
  llamadas, así que una sentencia que no se detiene se abandona junto con su sesión, y la app abre una
  nueva.
- DDL de objetos solo en **Trino** (`SHOW CREATE`). Snowflake, BigQuery, Databricks y otros drivers
  no muestran DDL.
- Las claves foráneas que apuntan a una tabla no se listan (habría que leer las claves de todas las
  tablas).
- Exportaciones, e importaciones de archivos (las importaciones con tipos se prueban en Trino).

## Qué no funciona

- **Sin confirmación manual**: cada sentencia se confirma sola, y al desactivar el autocommit se dice
  que la confirmación manual no está soportada con un driver ADBC.
- **Sin explain**, y sin pestañas de administración.
- **Sin edición de datos en la cuadrícula**: un guardado comprueba cada cambio en una transacción, y
  estas conexiones no tienen. Los resultados siguen siendo de solo lectura; usa SQL para los cambios.
- El modo terminal no está disponible.

## Agentes

`run_query`, `sample_rows` y las demás herramientas funcionan, dentro de las comprobaciones. Sin una
sesión de solo lectura en el servidor, los agentes de solo lectura dependen solo de la comprobación de
la sentencia. Las sentencias que actúan fuera de ella se preguntan antes de ejecutarse: `query()` de
Trino y las funciones `SYSTEM$` de Snowflake. Consulta [Agentes y la CLI](../../../../AGENTS.md).

## Límites conocidos

- Un inicio de sesión en un almacén en la nube distinto de los campos de arriba (par de claves,
  inicio de sesión único) no está en el formulario; el driver genérico acepta las opciones propias del
  driver.
- Snowflake, BigQuery y Databricks están en el código y comparten la capa de drivers con Trino, pero
  no tienen pruebas contra los servicios reales.

## Probado en

La batería de pruebas de conformidad ejecuta **Trino 484** con su driver descargado (tipos,
resultados largos, cancelar, metadatos, errores, sentencias que terminan con un terminador,
importaciones con tipos, y filas y UUID) y el **driver ADBC genérico** con el driver ADBC de Apache
SQLite y con el de DuckDB. Snowflake, BigQuery y Databricks no están probados.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

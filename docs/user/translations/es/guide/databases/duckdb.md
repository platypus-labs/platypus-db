<!-- source: 728e6511c5f7 -->
[← Bases de datos](README.md)

# DuckDB

DuckDB se ejecuta dentro de la app, como SQLite: sin servidor, host, usuario ni contraseña. A
diferencia de SQLite, la app no incluye la biblioteca de DuckDB; la **descarga la primera vez** que se
abre una base de datos DuckDB o un archivo de datos.

## Primer uso: la biblioteca de DuckDB

Cuando eliges **DuckDB** para una conexión nueva y la biblioteca no está en este equipo, se abre una
ventana sobre el formulario: *¿Descargar DuckDB?* Muestra el paquete (DuckDB 2.0.0, de la versión de
DuckDB en GitHub, licencia MIT), su tamaño y su origen, y comprueba la descarga con un SHA-256.
**Descargar** la guarda en la carpeta de datos de la app, una vez para todas las bases de datos y
archivos DuckDB; **Cancelar** deja el formulario utilizable, y la oferta vuelve en *Probar la
conexión* y en la primera apertura. *Ajustes › Drivers* lista la biblioteca y la elimina.

La app la descarga para Windows (x64, ARM64), Linux (x64, ARM64) y macOS (una biblioteca para ambos
procesadores). La app usa exactamente la versión con la que se compiló (2.0.x) y ofrece esa si hay
otra instalada. Sin conexión, con la biblioteca ausente, la conexión no se puede abrir.

## Conectar

Elige **DuckDB** e indica la ruta del archivo de la base de datos, o pega una URL:

| Pegas | Significa |
|---|---|
| `duckdb:///home/ana/analytics.duckdb` | ese archivo, que se crea si falta (su carpeta debe existir) |
| `duckdb:///C:/data/analytics.duckdb` | un archivo en una unidad de Windows |
| `duckdb:///:memory:` | la base de datos en memoria que comparten las sesiones de la app |
| `:memory:<name>` como ruta | una base de datos en memoria propia, con nombre |

**Archivos de datos como bases de datos.** Una ruta que termina en `.csv`, `.tsv`, `.json`, `.ndjson`,
`.jsonl`, `.parquet`, `.xlsx`, `.xlsm`, `.xls` u `.ods` se abre como una base de datos en memoria
sobre ese archivo (también puedes soltar un archivo en la ventana). CSV, TSV, JSON, NDJSON y Parquet
pasan a ser una vista con el nombre del archivo, que DuckDB vuelve a leer en cada consulta; cada hoja
de un libro pasa a ser una tabla, cargada una vez. Un CSV o TSV se comprueba antes (cada línea debe
tener tantos valores como el encabezado), se detecta su codificación de texto (o la indicas tú), y las
opciones de importación permiten omitir las líneas erróneas.

Las extensiones no se descargan ni se cargan por sí solas; solo están las que vienen integradas en la
biblioteca (Parquet, JSON).

## Privilegios

Los del sistema operativo: acceso de lectura al archivo, y de escritura a él y a su carpeta para
cambiar algo. Solo un proceso a la vez puede abrir un archivo DuckDB para escribir, así que mientras la
app tiene un archivo abierto otro programa (la línea de comandos `duckdb`, por ejemplo) no puede usarlo;
las sesiones de la app sobre un archivo comparten una instancia. Por eso no se ofrece el modo
terminal.

## Qué funciona

- Edición de datos en la cuadrícula, por clave primaria (o una clave única en columnas no nulas, o una
  clave virtual). Un archivo de datos se lee en memoria: una vista de CSV, JSON o Parquet no es una
  tabla y no se puede editar, y las ediciones en las hojas de un libro no se escriben de vuelta en el
  archivo.
- Confirmación manual y rollback; cancelar una sentencia (`duckdb_interrupt`).
- Explain muestra el plan (`EXPLAIN (FORMAT JSON)`); **Explain analyze** ejecuta la sentencia en una
  transacción que revierte, y se rechaza mientras tengas una transacción abierta.
- Pestaña Parámetros: los ajustes de DuckDB. Sin pestañas de sesiones, bloqueos, panel, usuarios ni
  sentencias.
- Búsqueda de metadatos, DDL de objetos, herramientas de esquema. DuckDB modifica poco en el sitio,
  así que cambiar una tabla la crea de nuevo y copia sus filas.
- Las importaciones pasan por el appender de DuckDB, en la transacción de la sesión.

## Agentes

Un agente de solo lectura, y una conexión de solo lectura, abren el archivo de la base de datos en
solo lectura, cuando ninguna otra sesión de la app lo tiene ya abierto; una base de datos en memoria o
un archivo de datos nunca es de solo lectura, y la comprobación de la sentencia impide allí las
escrituras. Las sesiones DuckDB de los agentes no pueden leer ni escribir otros archivos ni cargar
extensiones, y cierran el archivo después de cada llamada para que la app pueda abrirlo.
`query_across_databases` se ejecuta en un DuckDB propio que la app descarga igual que este. Consulta
[Agentes y la CLI](../../AGENTS.md).

## Límites conocidos

- Los archivos Excel se leen con el lector propio de la app, no con la extensión Excel de DuckDB.
- No se carga una versión de DuckDB distinta de la 2.0.x.

## Probado en

La batería de pruebas de conformidad se ejecuta en un archivo DuckDB, con la biblioteca que coincide
con los bindings de la app: transacciones, planes, edición de tablas, valores especiales, números de
128 bits y sin signo, y horas con zona.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

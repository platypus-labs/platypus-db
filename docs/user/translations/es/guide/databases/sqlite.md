<!-- source: eeb36912ad18 -->
[← Bases de datos](README.md)

# SQLite

SQLite se ejecuta dentro de la app (`rusqlite` con SQLite integrado): sin servidor, sin host, sin
usuario, sin contraseña, nada que descargar.

## Conectar

Elige **SQLite** e indica la ruta del archivo, o pega una URL:

| Pegas | Significa |
|---|---|
| `sqlite:///home/ana/app.db` | el archivo `/home/ana/app.db` |
| `sqlite:///C:/data/app.db` | un archivo en una unidad de Windows |
| `sqlite:///:memory:` | una base de datos en memoria |
| `file:app.db?mode=ro` en el campo de la ruta | una URI de SQLite, que se pasa a SQLite tal como está escrita |

El archivo se crea, como una base de datos vacía, cuando no existe pero su carpeta sí. Una carpeta que
falta es un error (lo más probable es una errata o una unidad sin montar), y también lo es una ruta
relativa que no existe: indica la ruta completa. Un archivo que no es una base de datos SQLite se
rechaza al conectar.

Una conexión de solo lectura (y un agente de solo lectura) abre el archivo en solo lectura, así que
el propio SQLite rechaza una escritura. Los ajustes de SSL, SSH y proxy no se aplican a una base de
datos embebida.

## Privilegios

Los del sistema operativo: la app necesita leer el archivo, y escribir en él y en su carpeta (para el
journal) para cambiar algo. Si el archivo está ocupado, se espera antes de dar un error.

## Qué funciona

- Edición de datos en la cuadrícula, por clave primaria (o una clave única en columnas no nulas, o una
  clave virtual). Una columna `INTEGER PRIMARY KEY` cuenta como clave aunque SQLite la declare como
  anulable.
- Confirmación manual y rollback; cancelar una sentencia en ejecución (`sqlite3_interrupt`).
- Explain muestra solo el plan de consulta de SQLite: no hay Explain analyze.
- Pestaña Parámetros: los pragmas que importan, con los valores por defecto documentados de SQLite.
- Sin pestañas de sesiones, bloqueos, panel, usuarios ni sentencias.
- El editor de tablas, el diagrama ER y la comparación. SQLite modifica poco en el sitio, así que
  cambiar una tabla la crea de nuevo y copia sus filas; la comparación y la reconstrucción conservan
  los índices de expresión y parciales, las claves diferidas y el contador de `AUTOINCREMENT`.
- Búsqueda de metadatos, DDL de objetos, y exportaciones, importaciones y copias como en cualquier
  base de datos SQL.
- El modo terminal ejecuta `sqlite3`, si está en el PATH.

## Agentes

Los agentes de solo lectura y las conexiones de solo lectura abren el archivo en solo lectura, y
ejecutan sus consultas con `query_only`. `estimate_change` no tiene estimación de filas que dar en
SQLite. Consulta [Agentes y la CLI](../../AGENTS.md).

## Límites conocidos

- Otro proceso puede tener el archivo: la app espera hasta 5 segundos con un archivo ocupado, y luego
  SQLite lo notifica.
- La sentencia de un agente que llama a `load_extension(…)` se pregunta antes, como un cambio.

## Probado en

La batería de pruebas de conformidad se ejecuta en el SQLite integrado en la app, con transacciones,
planes, edición de tablas, la reconstrucción que conserva `AUTOINCREMENT` y archivos abiertos en solo
lectura.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

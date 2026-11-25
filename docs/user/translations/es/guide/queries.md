<!-- source: d4ffd9bf1ac2 -->
[← Guía del usuario](README.md)

# Ejecutar consultas y leer los resultados

![Autocompletado de JOIN a partir de las claves foráneas](../../screenshots/autocomplete.png)

## Ejecutar una sentencia

1. Haz doble clic en una conexión (o en una tabla) para abrir un editor SQL.
2. Escribe una sentencia. `Ctrl+Space` completa palabras clave, tablas y columnas; después de
   `JOIN ` te propone las tablas relacionadas a partir de las claves foráneas.
3. Pulsa `Ctrl+Enter` para ejecutar la sentencia bajo el cursor (o la selección), o `Alt+X` para
   ejecutar todo el script. `Ctrl+\` ejecuta en una pestaña de resultados nueva.
4. `Ctrl+.` cancela una sentencia en ejecución.

Las sentencias terminan en un `;`, o en una línea en blanco antes de una palabra que empieza otra.
Cuando una sentencia falla, el error se señala en el texto y el panel de errores ofrece **Ir al
error**. `Ctrl+Shift+F` formatea el script; `Ctrl+S` lo guarda como archivo `.sql`.

## Parámetros y variables

`:id` y `?` en una sentencia, y las variables `${name}`, se piden en un diálogo antes de ejecutar y
se guardan en un panel Variables. El valor de una `${name}` entra en el SQL tal cual, así que una
sentencia con variables se revisa de nuevo ya rellena (consulta [Seguridad](safety.md)).

## Leer los resultados

- La cuadrícula lee las filas de página en página. Haz clic en el encabezado de una columna para
  ordenar: escribe el `ORDER BY` en el editor y vuelve a ejecutar; un segundo clic lo invierte, un
  tercero devuelve el orden propio de la consulta.
- Abre una tabla desde el navegador para recorrerla; un `WHERE` escrito en el editor la filtra.
- `F7` abre el panel de valores: texto largo, JSON y XML con formato, hexadecimal e imágenes de las
  columnas binarias.
- Una celda que contiene un archivo (un PDF, una imagen, un ZIP y otros, reconocidos por sus bytes)
  muestra su tipo y tamaño, como `PDF · 2.3 MB`. El menú de la cuadrícula ofrece **Guardar PDF…** (o
  el tipo del archivo) y, con varias celdas de una columna, **Guardar N archivos en una carpeta…**.
  El panel de valores tiene el mismo botón.
  Una celda de texto que contiene un archivo (una URL `data:`, base64 o hexadecimal) no lleva
  etiqueta en la cuadrícula, porque eso obligaría a leer todas las celdas de texto para saberlo: el
  menú de la cuadrícula y el panel de valores lo muestran, para las celdas que selecciones.
- `Ctrl+F` busca en las filas cargadas; `Ctrl+Shift+C` copia la selección con los encabezados.
- Un resultado puede pasar de la cuadrícula a un gráfico desde la barra de estado.

![Celdas que contienen archivos, con su tipo y tamaño](../../screenshots/guide/file-cell.png)

## Entender una consulta lenta

Pulsa `Ctrl+Shift+E` en una sentencia para ver su plan. Consulta
[FEATURES.md](../FEATURES.md#query-plans-and-performance).

## Más

Los scripts guardados, los snippets, el historial, el modo vim, ejecutar un script en varias
conexiones, el constructor de consultas y el análisis de datos están en
[FEATURES.md](../FEATURES.md#sql-editor). La cuadrícula de resultados está en
[FEATURES.md](../FEATURES.md#results-and-the-data-viewer).

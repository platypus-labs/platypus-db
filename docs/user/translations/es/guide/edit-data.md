<!-- source: 19e81b2b8673 -->
[← Guía del usuario](README.md)

# Editar datos

![Cambios pendientes en el editor de datos](../../screenshots/data-editor.png)

## Cambiar filas en la cuadrícula

1. Abre una tabla desde el navegador, o ejecuta una consulta que lea una sola tabla.
2. Haz doble clic en una celda, pulsa `F2` o simplemente escribe para cambiarla. `Shift+Delete`
   pone las celdas seleccionadas en NULL.
3. `Insert` añade una fila, `Ctrl+D` duplica la seleccionada, `Delete` la borra.
4. Los cambios se quedan resaltados hasta que guardes. Usa **Vista previa del SQL** para leer las
   sentencias.
5. Pulsa `Ctrl+S` (o **Guardar los cambios**) para guardarlos todos, o ninguno si uno falla.
   `Ctrl+Z` deshace los cambios que aún no se han guardado.

Cada guardado deja un script de deshacer que devuelve las filas a su estado anterior. En modo de
confirmación manual, los cambios se unen a la transacción abierta.

## Cuando la cuadrícula es de solo lectura

Una tabla necesita una clave primaria, o una clave única en columnas no nulas, o una clave virtual
que tú elijas; si no, la cuadrícula explica por qué no se puede editar. Una conexión también puede
prohibir la edición de datos (página **Apariencia y seguridad** de sus ajustes): entonces las filas
solo cambian con SQL.

## Editar documentos, claves y filas sin SQL

Los resultados de Redis, Elasticsearch, Cassandra y MongoDB también se editan, con sus propios
comandos en lugar de SQL: **Vista previa del SQL** los muestra. En MongoDB, el resultado de un
`find` simple sobre una colección se puede editar por `_id`. Cada cambio es un `updateOne`,
`insertOne` o `deleteOne`, escrito con el tipo BSON de la columna; un número modificado conserva el
tipo numérico que guarda el documento cuando ese tipo lo representa con exactitud. Editar `_id`,
columnas de tipos mixtos y campos con un `.` en el nombre se rechaza, con el motivo. En un replica
set un guardado es todo o nada; en un servidor standalone los comandos se ejecutan uno tras otro y
el error dice cuántos se guardaron. Consulta
[MongoDB](databases/mongodb.md#editar-documentos-en-la-cuadrícula).

## Editar muchas celdas a la vez

Selecciona las celdas y usa **Editar la selección** en el menú de la cuadrícula: un valor, NULL o
DEFAULT en cada celda, buscar y reemplazar en las celdas de texto, o una secuencia de números.
`Ctrl+Alt+D` rellena hacia abajo desde la primera fila (`⌃⇧D` en macOS). Solo cambian las filas
cargadas en la cuadrícula, y el resultado son cambios pendientes que puedes previsualizar, guardar o
deshacer juntos.

Pega desde una hoja de cálculo con `Ctrl+V` sobre las celdas, o con `Ctrl+Shift+V` como filas
nuevas.

## Cargar un archivo en una celda

Desde el menú de la cuadrícula, o el botón `…` del editor de la celda: los bytes van a las columnas
binarias y el texto UTF-8 a las columnas de texto. Los archivos de más de 50 MB piden confirmación
antes; los de más de 200 MB se rechazan.

## Filas propuestas por un agente

Cuando un agente llama a `propose_edits`, la tabla se abre en su visor de datos con las filas
propuestas como cambios pendientes, coloreadas como las tuyas. No se escribe nada hasta que las
guardes; descártalas para rechazarlas. Consulta [Agentes](agents.md).

## Más

Las claves foráneas, los arrays, el editor de tablas (columnas, claves, índices) y los datos
ficticios están en [FEATURES.md](../FEATURES.md#data-editor). Las conexiones de producción piden
confirmar cada cambio antes: consulta [Seguridad](safety.md).

<!-- source: 81c6c1d2c8d1 -->
[← Bases de datos](README.md)

# MongoDB

El driver es el driver oficial de Rust de MongoDB (con rustls, así que sin OpenSSL). Abre MongoDB 5.0
y posteriores. El editor usa la **sintaxis de mongosh**, no SQL. El driver es parte de la app; no se
descarga nada.

## Conectar

| Pegas | Se interpreta como |
|---|---|
| `mongodb://user:pass@host:27017/shop?authSource=admin&replicaSet=rs0` | la forma URL; puerto por defecto 27017 |
| `mongodb://a:27017,b:27018/?authSource=admin` | una lista de semillas, que se guarda entera en el campo Host |
| `mongodb+srv://user:pass@cluster0.example.net/shop` | SRV: los hosts salen del DNS |

Lo que no cabe en los campos del formulario va en el campo **Base de datos**, después de un `?`, como
opciones de la cadena de conexión: `shop?authSource=admin&replicaSet=rs0`, con `srv=true` para
`mongodb+srv`. El nombre antes del `?` es la base de datos en la que se ejecutan las sentencias hasta
que `use` la cambia (`test` si está vacío).

- **Inicio de sesión**: un usuario y una contraseña, autenticados contra `admin` salvo que
  `authSource` diga otra cosa. Una URL pegada cuya ruta nombra una base de datos la usa como fuente de
  autenticación, como indica la especificación de la cadena de conexión. Las opciones que contienen un
  secreto (una contraseña en `authMechanismProperties`, por ejemplo) no se conservan al pegar una URL;
  el formulario lista lo que dejó fuera.
- **TLS**: `tls=true` o `ssl=true` en la URL significa `verify-full`; `tls=false` significa
  `disable`. MongoDB no tiene TLS oportunista, así que `prefer` (el valor por defecto) mantiene lo que
  implica la cadena de conexión: TLS activado para `mongodb+srv`, desactivado en los demás casos.
  `require` activa TLS y acepta cualquier certificado; `verify-ca` y `verify-full` comprueban ambos la
  cadena y el nombre del host. `sslrootcert` (o `tlsCAFile`) nombra el archivo de tu CA; un
  certificado de cliente se puede dar en el formulario, o con `tlsCertificateKeyFile` en el campo
  Base de datos. Las opciones de TLS de la cadena de conexión se conservan en todos los modos.
- **A través de un túnel SSH** solo se llega al servidor que hay detrás (`directConnection=true`).

## Privilegios

El navegador lista las bases de datos y las colecciones que el inicio de sesión puede listar, y toma
muestras de documentos para mostrar los campos. No se lee nada más para administrar: no hay pestañas
de administración. Las sentencias que escribas necesitan los roles que necesiten.

## Qué funciona

- Sentencias en sintaxis de mongosh, con los valores del shell (`ObjectId(...)`, `ISODate(...)`,
  `NumberLong(...)`, `/regex/i`) y Extended JSON:
  - **Lectura**: `find` con `sort`, `limit`, `skip`, `projection`, `collation`, `hint`, `min`, `max`,
    `maxTimeMS`, `readConcern` y similares, `findOne`, `aggregate`, `countDocuments`,
    `estimatedDocumentCount`, `distinct`.
  - **Escritura**: `insertOne`/`insertMany`, `updateOne`/`updateMany`, `replaceOne`,
    `deleteOne`/`deleteMany`, `bulkWrite`, `findOneAndUpdate`/`findOneAndReplace`/`findOneAndDelete` y
    `findAndModify`, con sus opciones (`arrayFilters`, `collation`, `hint`, `let`, `upsert`,
    `ordered`, `writeConcern`). Una opción que PlatypusDB no conoce se rechaza por su nombre, nunca se
    descarta.
  - **Índices y colecciones**: `createIndex`, `createIndexes`, `dropIndex`, `dropIndexes`, `hideIndex`,
    `unhideIndex`, `getIndexes`, `createCollection`, `createView`, `drop`, `renameCollection`,
    `convertToCapped`, `stats`, `validate`, `latencyStats`, `dataSize`, `storageSize`, `totalIndexSize`,
    `totalSize`, `isCapped`, `exists`.
  - **Planes**: `.explain()` después de una sentencia o `db.c.explain().find(…)` antes de ella, con
    una verbosidad, y el **Explain** del editor. Un plan nunca aplica una escritura.
  - **El servidor**: `db.runCommand`, `db.adminCommand`, `db.aggregate`, `db.serverStatus`, `db.currentOp`,
    `db.killOp`, el profiler, usuarios y roles (`db.createUser` y similares), `rs.status`, `rs.conf`,
    `sh.status`, `show dbs`, `show collections`, `show users`, `show roles`, `show profile`, `show logs`
    y `use`.
- Los resultados son una cuadrícula de los campos de primer nivel, con el documento seleccionado como
  árbol a su lado (`F7`). El navegador lista las bases de datos, las colecciones, los campos obtenidos
  por muestreo de los documentos y los índices; el editor completa colecciones, métodos y campos.
- Cancelar detiene una sentencia en ejecución; un resultado abandonado libera la conexión.
- **Confirmación manual** en un replica set o un clúster con sharding (consulta
  [Transacciones](#transacciones)).
- **Edición de documentos en la cuadrícula** (consulta
  [Editar documentos en la cuadrícula](#editar-documentos-en-la-cuadrícula)).
- La definición de una colección como DDL.
- **Generar datos…** rellena una colección con documentos ficticios.
- El mismo detalle está en [FEATURES.md](../../../../FEATURES.md#mongodb).

## Qué no funciona

- **Solo algunos resultados se pueden editar en la cuadrícula**: los documentos de una colección
  (consulta [Editar documentos en la cuadrícula](#editar-documentos-en-la-cuadrícula)). Todos los
  demás resultados, como una agregación, una vista o un `findOne`, siguen siendo de solo lectura.
  Cambia esos documentos con `updateOne`, `insertOne` y similares.
- **Sin transacciones en un servidor standalone**: MongoDB las ejecuta solo en un replica set o un
  clúster con sharding. La confirmación manual se rechaza allí, y lo dice.
- **Sin JavaScript**: el editor ejecuta sentencias, así que `forEach`, `map`, `next`, `mapReduce`, los
  constructores de bulk antiguos (usa `bulkWrite`) y las variables se rechazan, y lo dice. `watch`
  también se rechaza: un change stream se apoderaría de la conexión. Un `.comment()` en una lectura se
  omite: la propia app lo usa para encontrar la operación que cancelar.
- `bulkWrite` no es atómico, como en mongosh: lo que se ejecutó antes de un fallo se queda, y el error
  dice hasta dónde llegó. Los inserts contiguos viajan juntos, cualquier otra escritura en su propio
  viaje.
- Sin SQL: sin editor de tablas, diagrama ER, comparación de esquemas, constructor de consultas,
  importación SQL, instantáneas antes de cambios ni modo terminal (`mongosh` toma la contraseña solo
  en su línea de comandos).

## Editar documentos en la cuadrícula

La cuadrícula edita el resultado de un `find` simple sobre una colección: el visor de datos, o una
sentencia que escribas, como `db.orders.find({ status: 'paid' }).sort({ _id: 1 }).limit(20)`. La
colección se puede nombrar con `getCollection("my orders")` o `db.getSiblingDB("shop").orders`. Tras
un `use`, la colección es la de la base de datos a la que cambiaste. Una proyección vale cuando solo
lista campos que incluir y conserva `_id`. Las vistas, las agregaciones, `findOne`, `distinct`, los
recuentos, una proyección sin `_id` y una proyección que deja campos fuera siguen siendo de solo
lectura.

Un documento se localiza por su `_id`. Un resultado sin `_id` es de solo lectura. **Vista previa del
SQL** muestra el mongosh que se ejecutará, una sentencia por documento:

- un documento editado es `updateOne({ _id: … }, { $set: { … } })`. Una celda NULL escribe `null`.
  Cuando cambia un número, la actualización es en cambio un pipeline, `[{ $set: { … } }]`: el número
  se escribe con un `$switch` sobre el tipo que guarda el documento (consulta más abajo), y cualquier
  otro valor como un `$literal`, para que una cadena como `$total` o un documento con claves `$` se
  guarde tal cual;
- un documento nuevo es `insertOne({ … })`. Las celdas que se dejan en NULL no se envían. Sin un
  `_id`, la app crea un `ObjectId`;
- un documento borrado es `deleteOne({ _id: … })`.

Los valores se escriben con el tipo BSON de su columna: `bool`, `string`, `objectId`, `date`, y
`document` y `array` a partir del JSON que muestra la cuadrícula. Una fecha se escribe como
`2024-01-02 10:30:00`, con o sin la zona horaria (UTC si no la lleva). Los números dentro de un
documento o un array conservan el tipo que les da JSON: un número entero es un int (un long más allá
de 32 bits), un número con fracción o exponente es un double.

Un número conserva el tipo que guarda el documento, `int`, `long`, `double` o `decimal`, siempre que
ese tipo represente con exactitud el valor nuevo: el tipo de una columna es solo lo que comparten sus
documentos (ints y longs dan una columna `long`, cualquiera de ellos con doubles una `double`), así
que no dice qué guarda un documento concreto. Un int editado a `7` sigue siendo un int, un long un
long, un double un double. Un valor que el tipo guardado no puede contener toma el tipo de la columna,
o el primer tipo numérico que lo contiene: `3.5` en un campo int pasa a ser un double, `3000000000` un
long. Un long más allá de 2^53 nunca se escribe como double, lo que perdería dígitos. No se convierte
nada en el servidor: la app solo escribe el número en los tipos que sabe que lo representan con
exactitud.

Esto se rechaza antes de que se ejecute nada, con el motivo:

- un cambio de `_id`: MongoDB nunca lo cambia. Borra el documento y vuelve a insertarlo;
- una columna de tipos mixtos (`mixed`): el texto de un valor no distingue una cadena de un número;
- los datos binarios y los demás tipos que la cuadrícula no puede volver a escribir con exactitud
  (expresiones regulares, timestamps, JavaScript);
- un campo con un `.` en el nombre o un nombre que empieza por `$`: `$set` lo leería como una ruta o
  un operador;
- un valor que no cabe en su tipo, como `many` en una columna `int` o un array en una columna
  `document`.

Cada sentencia debe cambiar exactamente un documento. Si una no encuentra ninguno, porque el
documento se borró o su `_id` cambió desde que la cuadrícula lo leyó, el guardado se detiene y dice
qué comando.

**Todo o nada necesita un replica set.** En un replica set o un clúster con sharding, un guardado se
ejecuta en una transacción propia y confirma solo si todos los comandos funcionaron. Si tienes una
transacción abierta (confirmación manual), el guardado se ejecuta en ella y espera a Confirmar. Si un
comando falla, MongoDB no tiene savepoints: se revierte toda la transacción, con los cambios que aún no
habías confirmado, y el error lo dice. En un servidor standalone no hay transacciones: los comandos
se ejecutan uno tras otro, y un fallo detiene el resto. Los comandos anteriores se quedan guardados, y
el error dice cuántos.

El script de deshacer que sigue a un guardado devuelve los documentos a su estado anterior: `$set` de
los valores antiguos para las ediciones, `deleteOne` para los documentos nuevos, `insertOne` de los
campos cargados para los borrados. Las líneas que empiezan por `//` dicen lo que no puede restaurar
con exactitud: los campos que el resultado no mostraba, las columnas de tipos que la cuadrícula no
puede escribir, los campos que eran NULL (vuelven como `null`, o se omiten), y los números cuyo tipo
pudo cambiar la edición (vuelven como números del tipo que tienen entonces, con el mismo `$switch`).

## Transacciones

La confirmación manual funciona en un replica set o un clúster con sharding (un `mongos`); basta un
replica set de un solo nodo. En un servidor standalone se rechaza: el botón **Auto-commit** está
desactivado y dice por qué. Una conexión de producción a un replica set se abre en confirmación
manual, con Confirmar y Revertir. La conexión mantiene una sola sesión: el primer cambio después de
pasar a confirmación manual, o después de un Confirmar o un Revertir, abre una transacción en ella (un
insert, update o delete, `createCollection`, `createIndex`, un guardado de la cuadrícula), y Confirmar
o Revertir la termina. Las lecturas anteriores no abren ninguna: ven lo confirmado, como en
autocommit, así que navegar por los datos nunca deja una transacción abierta para que el servidor la
termine al agotarse su tiempo de vida. Una vez abierta una transacción, tus lecturas se ejecutan en
ella y ven sus escrituras; las otras sesiones solo las ven tras Confirmar.

- **En la transacción**, una vez abierta una (las lecturas) o abriéndola (el resto): `find`,
  `findOne`, `aggregate`, `countDocuments`, `distinct`, los inserts,
  updates, replaces y deletes, `bulkWrite`, los métodos `findOneAnd…`, `createCollection` y
  `createIndex`, y `db.runCommand` o `db.adminCommand` con un comando que lee o escribe documentos
  (`find`, `aggregate`, `insert`, `update`, `delete`, `findAndModify`, …). MongoDB admite un índice en
  una transacción solo en una colección creada en la misma transacción: en una que ya existe, el
  servidor lo rechaza y termina la transacción.
- **Fuera de ella**, como en modo autocommit, sin tocarla: lo que lee metadatos o habla con el
  servidor. `show …`, `use`, `db.stats()`, `db.getCollectionNames()`, `getIndexes`, las estadísticas
  de la colección, `estimatedDocumentCount`, `.explain()`, los demás comandos de `db.runCommand` y
  `db.adminCommand`, `rs.*`, `sh.*`, usuarios y roles, el profiler, el navegador y el autocompletado.
  Las lecturas de las bases de datos `admin`, `config` y `local` y de las colecciones `system.*`
  también se ejecutan fuera.
- **Rechazado en confirmación manual**: `drop`, `renameCollection`, `dropIndex`, `dropIndexes`,
  `hideIndex`, `convertToCapped`, `createView`, un `createCollection` de series temporales, un
  `aggregate` con `$out` o `$merge`, y las escrituras en `admin`, `config`, `local` o `system.*`.
  MongoDB no los ejecuta en una transacción, y ejecutados fuera se aplicarían al instante, algo que
  Revertir no podría deshacer. El error lo dice; no se envía nada y la transacción abierta queda como
  estaba. Pasa a autocommit para ejecutarlos.
- Un resultado leído en una transacción sigue cargándose al desplazarte, por lotes, y entre medias
  pueden ejecutarse otras sentencias. Un resultado leído antes de que se abriera la transacción sigue
  cargándose fuera de ella. Sus columnas salen de sus primeros documentos. Tras Confirmar o Revertir
  no se puede leer el resto: ejecuta la sentencia de nuevo.

**Cuando el servidor termina una transacción.** MongoDB aborta una transacción que sigue abierta más
de `transactionLifetimeLimitSeconds`, 60 segundos por defecto. También aborta una con algunos
errores: una clave duplicada, un conflicto de escritura con otra sesión, una sentencia que rechaza en
una transacción, un límite de tiempo (`maxTimeMS`) alcanzado. Otros errores, como una etapa de
pipeline desconocida, la dejan abierta. Tras un error, PlatypusDB pregunta al servidor si la
transacción sigue existiendo. Cuando no existe, el error de la sentencia o de Confirmar dice que el
servidor terminó la transacción y que sus cambios se descartaron (con el límite de tiempo cuando fue
ese el motivo), la transacción se muestra como cerrada, y la siguiente sentencia abre una nueva.
Cancelar una sentencia en una transacción revierte la transacción: el servidor termina una
transacción cuya operación se mata, y lo que hizo la sentencia es desconocido.

## Agentes

`run_query`, `sample_rows` (`find({}).limit(n)`) y el resto funcionan con sentencias de mongosh; si
una sentencia no es una consulta simple, se pregunta antes. Rechazados en MongoDB: `explain`,
`get_er_diagram` y `query_across_databases`; `estimate_change` responde con una nota.

`propose_edits` propone documentos para insertar, actualizar o borrar en una colección (no en una
vista): la colección se abre en su visor de datos con un `find` de los `_id` propuestos y los cambios
pendientes, que guardas o descartas como tus propias ediciones. Un documento se nombra solo por
`_id`; un `_id` de tipo `ObjectId` se envía como sus 24 dígitos hexadecimales y se lee como un
`ObjectId` porque el `_id` de la colección lo es. Los valores se comprueban con las reglas del
guardado de la cuadrícula (consulta más arriba), antes de que se abra nada. Las columnas sensibles se
enmascaran solo por campo de primer nivel: un campo sensible dentro de un documento no se distingue.
Consulta [Agentes y la CLI](../../../../AGENTS.md).

## Límites conocidos

- Anterior a la 5.0: conecta con un aviso de que algunas funciones pueden no funcionar.
- El driver es un pool que se reconecta solo, así que una conexión perdida no se notifica como en las
  otras bases de datos.

## Probado en

La batería de pruebas de conformidad se ejecuta en MongoDB 5.0 y 8.0: información del servidor,
metadatos, cancelar, resultados abandonados, errores con su posición, documentos ficticios, y valores
en documentos posteriores que difieren del primero. Las transacciones se prueban en un replica set de
un solo nodo de MongoDB 8.0: confirmar, revertir, lecturas dentro y fuera, un resultado de varios
lotes, metadatos en confirmación manual, y una transacción que el servidor terminó por su límite de
tiempo.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

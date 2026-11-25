<!-- source: 641268954234 -->
[← Bases de datos](README.md)

# Apache Cassandra y ScyllaDB

Un solo driver, construido sobre el crate `scylla` (con rustls), habla CQL con ambas mediante el
protocolo nativo. El servidor dice cuál es, y la conexión lo muestra. El driver es parte de la app; no
se descarga nada. El editor usa CQL.

## Conectar

| Pegas | Se interpreta como |
|---|---|
| `cassandra://cassandra:s%40cret@node1:9043/shop` | sin cifrar, puerto 9043, keyspace `shop` |
| `scylla://node1` | el mismo driver, para ScyllaDB |
| `cassandras://…` y `scyllas://…` | con TLS (verificado: `verify-full`) |

El puerto por defecto es 9042. El campo **Host** es un punto de contacto, o varios pares `host:port`
separados por comas. El campo **Keyspace** (opcional) es el keyspace en el que empieza la sesión; el
navegador los lista todos en cualquier caso.

- **Un punto de contacto, o un túnel**: ese nodo solo atiende todas las sentencias. Un clúster
  anuncia las direcciones propias de sus nodos, que un contenedor, un NAT o un túnel ocultan, así que
  el driver no las sigue. Con varios puntos de contacto, el driver recibe todos y puede usar
  cualquiera.
- **Inicio de sesión**: un usuario y una contraseña, para el autenticador de contraseñas del
  servidor. Sin usuario la conexión es anónima.
- **TLS**: CQL no puede negociarlo, así que TLS está activado para las URL `cassandras://` y
  `scyllas://` y para los modos SSL de `require` en adelante; `prefer` es sin cifrar. `require` no
  comprueba el certificado; `verify-ca` y `verify-full` sí. Un archivo de CA y un certificado de
  cliente van en los campos de TLS.
- **Las sentencias no tienen timeout** propio del driver: una se ejecuta hasta que termina o la
  cancelas.

## Privilegios

El navegador lee `system_schema`, que comparten Cassandra 3 y posteriores y ScyllaDB; el inicio de
sesión ve lo que el servidor le deja leer allí. No hay pestañas de administración. Las escrituras
necesitan los permisos que pide CQL.

## Qué funciona

- Scripts CQL, divididos por el editor (se entienden las cadenas `$$ … $$`, los comentarios `//` y un
  `BEGIN BATCH … APPLY
  BATCH`), con resultados con tipos que se leen página a página al desplazarte, usando el estado de
  paginación del servidor.
- El navegador: keyspaces como esquemas, tablas, vistas materializadas, tipos definidos por el
  usuario, funciones y agregados, con las claves de partición y de clustering de cada tabla y sus
  índices; el DDL de cada objeto.
- **La cuadrícula edita filas**, por la clave primaria completa (columnas de partición y de
  clustering): una fila editada es un `UPDATE`, una fila nueva un `INSERT` que necesita todas las
  columnas de la clave, una fila borrada un `DELETE`. La vista previa muestra las sentencias, que se
  ejecutan una tras otra; si el servidor rechaza una, se detiene el resto. *Script de deshacer*
  devuelve los valores antiguos. Las columnas de la clave no se pueden cambiar (borra la fila e
  inserta otra), y los contadores se rechazan.
- Las transacciones ligeras (`IF …`) se notifican como tales con su resultado.
- Cancelar detiene un resultado que se está leyendo. Los errores conservan el mensaje del servidor.
- `USE keyspace` cambia el keyspace de la sesión, que el driver conserva.

## Qué no funciona

- **Sin transacciones**: cada sentencia surte efecto por sí sola, y no hay confirmación manual.
- **Las escrituras CQL son upserts y no informan de ningún recuento de filas**, así que una sentencia
  que no devuelve filas informa de que no cambió ninguna; una actualización de una fila que alguien
  borró mientras tanto la escribe de nuevo.
- El visor de datos no tiene barra de filtro y orden, y no muestra el recuento de filas.
- Sin explain, y sin herramientas SQL: sin editor de tablas, diagrama ER, comparación de esquemas,
  constructor de consultas, importación SQL, instantáneas antes de cambios ni modo terminal (`cqlsh`
  toma la contraseña solo en su línea de comandos).

## Agentes

`run_query` y `sample_rows` (un `SELECT … LIMIT n` de CQL) funcionan; una sentencia que cambia datos
se pregunta antes. Rechazados en Cassandra: `explain` y `propose_edits`; `estimate_change` responde
con una nota. Consulta [Agentes y la CLI](../../../../AGENTS.md).

## Límites conocidos

- Cassandra anterior a la 4.0 y ScyllaDB anterior a la 5.4 se rechazan al conectar, con "older
  than … the oldest version supported", a diferencia de las otras bases de datos, que conectan con un
  aviso.
- Sin joins, sin `OFFSET`, y las filas se localizan solo por su clave primaria, como lo tiene CQL.

## Probado en

La batería de pruebas de conformidad se ejecuta en Apache Cassandra 5 y ScyllaDB 2025.1, sin inicio de
sesión: información del servidor, scripts, cada tipo como columna con tipo, paginación, transacciones
ligeras, errores, cancelar, el catálogo y su DDL (también funciones y agregados), guardar los cambios
de la cuadrícula y deshacerlos, rechazo de un inicio de sesión erróneo o ausente, y una dirección de
túnel. Cassandra 4.0 y ScyllaDB 5.4 son las versiones más bajas que acepta el driver y la batería no
las ejecuta.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

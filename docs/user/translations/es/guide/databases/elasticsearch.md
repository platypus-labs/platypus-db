<!-- source: 2a42aace96dc -->
[← Bases de datos](README.md)

# Elasticsearch y OpenSearch

Un solo driver habla con ambos mediante su API HTTP (`reqwest` con rustls). El servidor dice cuál es al
conectar, y la conexión lo muestra (`Conectado: OpenSearch 2.17.1`). El driver es parte de la app; no
se descarga nada.

## Conectar

| Pegas | Se interpreta como |
|---|---|
| `elasticsearch://user:pass@host:9200/logs-*` | HTTP sin cifrar; la ruta es un patrón de índices |
| `elasticsearchs://user:pass@host:9200` | HTTPS (verificado: `verify-full`) |
| `opensearch://…` y `opensearchs://…` | lo mismo, recordado como OpenSearch para que la URL se escriba de vuelta así |

El puerto por defecto es 9200. El campo **Base de datos** es un patrón de índices opcional (`logs-*`,
o varios separados por comas) que acota lo que lista el navegador; vacío lista todos los índices que
no están ocultos.

- **Inicio de sesión**: un usuario y una contraseña usan la autenticación Basic. Con el usuario vacío,
  el campo de contraseña es una **clave de API** (el valor `encoded`), enviada como
  `Authorization: ApiKey …`.
- **TLS**: HTTPS para las URL `elasticsearchs://` y `opensearchs://` y para los modos SSL de `require`
  en adelante; `elasticsearch://` y `prefer` son HTTP sin cifrar. `require` no comprueba el
  certificado; `verify-ca` y `verify-full` sí. Un archivo de CA y un certificado de cliente van en los
  campos de TLS.

## Privilegios

Los que necesitan las peticiones. El navegador lee los índices, alias, data streams y mappings que el
inicio de sesión puede ver; no hay pestañas de administración.

## Qué funciona

- **Peticiones como las escribe la consola Dev Tools de Kibana**: una línea como
  `GET /orders/_search` (la barra es opcional; métodos `GET`, `POST`, `PUT`, `DELETE`, `HEAD`), y luego
  su cuerpo JSON. `_bulk` y `_msearch` aceptan cuerpos NDJSON que terminan en una línea en blanco o en
  la siguiente petición.
- **SQL**: todo lo demás es SQL (`_sql` en Elasticsearch, `_plugins/_sql` en OpenSearch), con columnas
  con tipo y el cursor del servidor para los resultados largos. Los errores conservan el motivo del
  servidor y un error de SQL señala su lugar en la sentencia.
- Una búsqueda da documentos como los resultados de MongoDB: una columna por cada campo de primer
  nivel, con el documento seleccionado como árbol (`F7`). Las agregaciones llegan como resultados
  propios.
- El navegador lista los índices como tablas (salud, shards y réplicas en el tooltip) y los alias y
  data streams como vistas. Un índice se abre como una búsqueda de todos sus documentos, leída de 1.000
  en 1.000 filas con un point in time y `search_after` (un scroll en OpenSearch), así que la lectura no
  se detiene en los 10.000 resultados. Un campo sin columna asignada va a una última columna `_other`
  como JSON.
- **La cuadrícula edita documentos**: una celda cambiada es un `_update` por `_id`, una fila nueva es
  un `_create` (el servidor rechaza un `_id` que ya existe) o un documento generado, una fila borrada
  es un `DELETE`. La vista previa muestra las peticiones, que se ejecutan una tras otra; si el
  servidor rechaza una, se detiene el resto, y las anteriores se quedan. *Script de deshacer* devuelve
  el `_source` antiguo (un documento con un `_id` generado hay que borrarlo a mano).
- `_delete_by_query` y `_update_by_query` sin una consulta (o con `match_all`), y `DELETE /_all`,
  preguntan antes, como un `DELETE` sin `WHERE`. Las conexiones de solo lectura solo ejecutan las
  peticiones y el SQL que leen.
- Cancelar detiene una búsqueda en el servidor. El DDL de objeto de un índice es la petición que lo
  crea de nuevo.
- Los detalles están en [FEATURES.md](../../FEATURES.md#elasticsearch-and-opensearch).

## Qué no funciona

- Sin transacciones (sin confirmación manual) y sin explain.
- Sin herramientas SQL: sin editor de tablas, diagrama ER, comparación de esquemas, constructor de
  consultas, importación SQL, instantáneas antes de cambios ni modo terminal.
- El visor de datos no tiene barra de filtro y orden.

## Agentes

`run_query` y `sample_rows` funcionan con peticiones de consola y SQL, y una petición que cambia datos
se pregunta antes. Rechazados en Elasticsearch y OpenSearch: `explain`, `get_er_diagram`,
`propose_edits` y `query_across_databases`. Consulta [Agentes y la CLI](../../AGENTS.md).

## Límites conocidos

- La app rechaza Elasticsearch anterior a la 7.10 y OpenSearch anterior a la 1.0: a diferencia de las
  otras bases de datos, la conexión falla con "older than … the oldest version supported".
- Probado en Elasticsearch 8 y OpenSearch 2, no en la 7.x.

## Probado en

La batería de pruebas de conformidad se ejecuta en Elasticsearch 8.15.3 (inicio de sesión `elastic`) y
OpenSearch 2.17.1: información del servidor, SQL con tipos y paginación, búsquedas que pasan de una
página, el catálogo y los mappings, guardar los cambios de la cuadrícula y deshacerlos, cancelar una
búsqueda lenta, y los errores del servidor.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

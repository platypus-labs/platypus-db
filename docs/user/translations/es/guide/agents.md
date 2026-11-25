<!-- source: cce13c1c788d -->
[← Guía del usuario](README.md)

# Usar PlatypusDB desde un agente

Claude Code, Codex, Cursor y otros clientes MCP pueden usar tus conexiones guardadas a través del
servidor MCP de PlatypusDB. Esta página es el camino corto; cada herramienta, límite y ajuste está
en la referencia, [AGENTS.md](../AGENTS.md).

## Conectar un agente

1. Abre Ajustes (`Ctrl+,`) › **Agentes** › **Configuración**.
2. Busca tu cliente en la lista (Claude Desktop, Claude Code, Codex, Cursor, Windsurf,
   Antigravity, Gemini CLI). Haz clic en **Instalar**: la app te pregunta, guarda una copia del
   archivo del cliente a su lado y escribe el servidor en él. Si tu cliente no está en la lista,
   copia la entrada a mano desde la misma página.
3. Reinicia el cliente. Él mismo inicia PlatypusDB con `--mcp`; no se abre ninguna ventana, y la app
   no tiene que estar abierta para leer.

## Elegir qué permite cada conexión

![Ajustes, Agentes, la página Seguridad](../../screenshots/guide/agents-security.png)

1. Ajustes › **Agentes** › **Seguridad** lista tus conexiones. El formulario de cada conexión tiene
   la misma opción, en **Agentes**, dentro de su página **Apariencia y seguridad**.
2. Elige **Desactivado**, **Solo lectura** (el valor por defecto) o **Lectura y escritura**.

Las conexiones de producción y las de solo lectura nunca pasan de solo lectura: el panel dice "No
disponible: las conexiones de producción y de solo lectura siguen en solo lectura." Una conexión
nueva de un proyecto compartido empieza en **Desactivado** en tu ordenador.

## Límites y enmascarado

- En **Seguridad**, **Límites para agentes** fija, por conexión, el máximo de filas, bytes y segundos
  de un resultado o una sentencia, las llamadas por minuto, y las filas o el coste de plan estimados
  de una lectura, por encima de los cuales la lectura se rechaza junto con su plan. Vacío significa
  sin más límite que los topes fijos.
- Las columnas sensibles se leen como `••••••` para los agentes, igual que en la cuadrícula con el
  enmascarado activo. Marca **Mostrar a los agentes los valores de las columnas sensibles** en una
  conexión para mostrarlas. Consulta [Seguridad](safety.md).

## Los cambios esperan tu respuesta

En una conexión de **Lectura y escritura**, cada cambio que un agente quiere ejecutar abre un
diálogo en la app con la sentencia, con los colores del editor y maquetada en las bases de datos SQL
(**Mostrar tal como lo envió el agente** muestra el texto que se ejecuta, al pie de la letra), las
filas que la base de datos espera cambiar y su plan. Haz clic en **Rechazar** (el agente lee tu
motivo), **Permitir una vez** o **Permitir en esta sesión**. Si pasan cinco minutos sin respuesta,
se rechaza, y con la app cerrada los cambios se rechazan.
Ajustes › **Seguridad** › **Preguntar antes de que los agentes cambien datos** desactiva las
preguntas.

Un agente también puede proponer, sin ejecutar nada:

- SQL con `propose_sql`: se abre en una pestaña de editor nueva, para que lo leas y lo ejecutes.
- Filas para insertar, actualizar o borrar con `propose_edits`: la tabla se abre en su visor de datos
  con ellas como cambios pendientes, para que las guardes o las descartes (consulta
  [Editar datos](edit-data.md)).

## Ver qué hicieron los agentes

Ajustes › **Agentes** › **Actividad** lista cada llamada a una herramienta por sesión, con un filtro
y **Exportar CSV**. Sus consultas están en el historial con el nombre del agente.

Encima de la lista, **Resumen por agente y sesión** (plegado hasta que lo abres) suma las llamadas
que seleccionan los filtros: una fila por agente y, debajo, una por sesión, con el periodo, las
llamadas, las tablas leídas, las filas devueltas, los cambios permitidos, los cambios rechazados y
los errores. **Exportar Markdown** y **Exportar HTML** lo guardan como un documento para compartir;
el HTML es un solo archivo, sin scripts y sin nada cargado desde fuera.

- Una **sesión** es un arranque del servidor MCP, tal como lo anota el registro (un cliente que
  sigue conectado durante días es una sola sesión).
- Un **cambio** es una sentencia que modifica datos o el esquema. Es *permitido* si se ejecutó y
  *rechazado* si el acceso de la conexión, un ajuste de seguridad o tu respuesta en la app dijeron
  que no. Una sentencia que falló por otro motivo cuenta solo como error.
- **Tablas leídas** es exacto para `sample_rows` y `profile_table` y para las fuentes de
  `query_across_databases`, y una estimación para una sentencia: las tablas nombradas después de
  `FROM` y `JOIN`. Una sentencia en un lenguaje que no es SQL (MongoDB, Redis, Elasticsearch,
  Cassandra) no nombra ninguna.
- Solo se guardan nombres y recuentos: ningún valor, y las sentencias siguen enmascaradas como en el
  registro.
- Las llamadas hechas antes de que esta versión guardara estas cifras muestran **–**, nunca un 0; las
  demás cifras cuentan solo las llamadas que las tienen, y el resumen lo dice.

## Cuando no funciona

Consulta [Solución de problemas](troubleshooting.md#un-agente-no-ve-una-conexión).

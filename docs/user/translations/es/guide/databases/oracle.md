<!-- source: fd0bf4386fb3 -->
[← Bases de datos](README.md)

# Oracle

El driver usa la biblioteca cliente de Oracle (ODPI-C, mediante el crate `oracle`), que se carga al
conectar. La biblioteca es Oracle Instant Client, que la app descarga en el primer uso.

## Primer uso: Instant Client

Cuando eliges **Oracle** para una conexión nueva y el cliente no está en este equipo, se abre una
ventana sobre el formulario: *Oracle requiere Oracle Instant Client*. Muestra el paquete (Basic
Light 23.26.4), su tamaño, su licencia (Oracle Free Use Terms and Conditions) y de dónde viene, y
comprueba la descarga con un SHA-256. **Descargar** lo guarda en la carpeta de datos de la app;
**Cancelar** deja el formulario utilizable, y la oferta vuelve en *Probar la conexión* y en la primera
apertura. *Ajustes › Drivers* lista el cliente y lo elimina.

- La app lo descarga en Windows (x64) y Linux (x64 y ARM64). En macOS no descarga nada: instala
  Instant Client desde Oracle y pon su carpeta en la ruta de bibliotecas.
- Si la app no ha descargado uno, usa un Instant Client o un cliente completo que el sistema pueda
  cargar.
- Windows necesita el Microsoft Visual C++ Redistributable, y Linux la biblioteca `libaio` (`libaio1`,
  `libaio1t64` o `libaio` según la distribución); cuando el cliente no se puede cargar, el error
  nombra la pieza que falta.

## Conectar

No hay URL `oracle://`; pega en su lugar una URL JDBC, como `jdbc:oracle:thin:@host:1521/service` (se
leen las formas thin y OCI), o rellena el formulario:

- **Host** y **puerto** (por defecto 1521), y **Nombre del servicio** en el campo Base de datos (por
  ejemplo `FREEPDB1`). Se escriben como una cadena Easy Connect, `host:port/service`.
- **Sin host**, el campo Base de datos es un nombre de servicio de red de `tnsnames.ora`, o un
  descriptor de conexión completo, escrito tal cual.
- **Usuario y contraseña.** Sin usuario, inicia sesión la cuenta del sistema operativo o un wallet de
  Oracle. El usuario se puede escribir al estilo de SQL*Plus, `sys as sysdba`, o puedes elegir el
  **Rol** (`SYSDBA`, `SYSOPER`, `SYSBACKUP`, `SYSDG`, `SYSKM`, `SYSASM`).
- **TLS**: los modos `disable` y `prefer` usan TCP sin cifrar; `require` y superiores usan `tcps://`
  con los certificados de confianza del sistema. `require` omite la comprobación del nombre del
  servidor; `verify-ca` y `verify-full` la hacen. Se aplica el timeout de conexión. A través de un
  túnel SSH se usa el extremo local del túnel.

## Privilegios

Navegar lee las vistas `ALL_*`, así que el inicio de sesión ve los objetos sobre los que tiene
derechos. Cancelar envía un break; si la sentencia sigue ejecutándose tras un par de segundos, la app
la cancela desde una segunda sesión con `ALTER SYSTEM CANCEL SQL`, que necesita ese privilegio, y si
eso falla abandona la sesión (su transacción se revierte y se abre una conexión nueva).

## Qué funciona

- Edición de datos en la cuadrícula, por clave primaria (o una clave única en columnas no nulas, o una
  clave virtual).
- Confirmación manual y rollback.
- Bloques PL/SQL: un script termina un bloque con una línea `/`. El texto de `DBMS_OUTPUT` vuelve como
  mensajes.
- Navegador (tablas, vistas, vistas materializadas, secuencias, sinónimos, funciones, procedimientos,
  triggers, tipos y paquetes), DDL de objetos y herramientas de esquema.
- Explain muestra el plan de `EXPLAIN PLAN` y `DBMS_XPLAN`, solo estimado: no hay Explain analyze.
- Los errores de compilación, los números y las horas con nanosegundos, JSON, vectores, cursores, LOB
  largos y columnas de archivo se leen tal como los guarda la base de datos.
- Sin pestañas de sesiones, bloqueos, panel, sentencias, usuarios ni parámetros.
- El modo terminal no está disponible.

## Agentes

Los agentes de solo lectura dependen solo de la comprobación de la sentencia: aquí Oracle no tiene
sesión de solo lectura en el servidor. Una sentencia que llama a un paquete de Oracle, como
`UTL_HTTP`, se pregunta antes de ejecutarse, como un cambio. Consulta
[Agentes y la CLI](../../../../AGENTS.md).

## Límites conocidos

- La app no compara la versión del servidor con un mínimo; el cliente que descarga es el 23.26.4.
- Para un servidor anterior a la 12, el navegador lee el diccionario de otra manera (sin
  `ORACLE_MAINTAINED`, sin `FETCH FIRST`). En las pruebas solo se ejecuta Oracle Free 23.26, así que
  ese camino no se ha ejercitado. Autonomous Database y otros servicios en la nube de Oracle tampoco
  están probados.
- Instant Client no se descarga en macOS.

## Probado en

La batería de pruebas de conformidad se ejecuta en Oracle Free 23.26 (contenedor
`gvenzl/oracle-free`), con Instant Client descargado o el del sistema: tipos, resultados largos,
errores que señalan el fallo, PL/SQL, navegador, confirmar y revertir, guardar muchas filas editadas,
cancelar (también un sleep de PL/SQL), planes, DDL que se vuelve a ejecutar, conectar como `SYSDBA`, y
comparación de esquemas. Las demás versiones no están probadas.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

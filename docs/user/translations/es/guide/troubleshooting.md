<!-- source: 25c359215e93 -->
[← Guía del usuario](README.md)

# Solución de problemas y preguntas frecuentes

Empieza con **Probar la conexión** en el formulario de la conexión: informa del fallo con el propio
mensaje del servidor, y nombra la versión del servidor cuando eso ayuda.

## Errores de conexión

### No se puede acceder al host

Comprueba primero el host y el puerto (el formulario dice "Falta el host." o "El puerto debe ser un
número del 1 al 65535."). Después mira lo que hay entre tú y el servidor: un túnel SSH, un proxy, un
reenvío de puerto, o un comando alrededor de la conexión que levanta una VPN (página **Red**). Cada
uno añade su propio mensaje, más abajo.

### TLS

En la página **Red**, **Modo SSL** decide qué se comprueba (consulta [Conectar](connect.md#tls)).

- Un servidor que rechaza una conexión sin cifrar necesita `require` o superior; un servidor con un
  certificado en el que tu ordenador no confía necesita `verify-ca` o `verify-full` con el
  **Certificado CA (PEM)**, o `require` para cifrar sin comprobar.
- "invalid certificate" e "invalid root certificates" significan un archivo PEM que no se puede
  leer: comprueba que es el certificado en sí, no una clave.
- "Un certificado de cliente requiere tanto el certificado como su clave": rellena **Certificado de
  cliente (PEM)** y **Clave de cliente (PEM)** juntos. "invalid client certificate or key" significa
  que no coinciden o que no son PEM.
- Redis, Elasticsearch y Cassandra toman TLS de un modo SSL de `require` o superior.

### SSH

Los errores empiezan con `SSH <user>@<host>:<port>:`.

- `no answer within 10 s` o `cannot reach <host>:<port>`: el servidor SSH (o el proxy que tiene
  delante) no es accesible desde aquí.
- `cannot read the key <path>`: el **Archivo de clave privada** no existe, no se puede leer o necesita
  su frase de paso.
- `no SSH agent is running`: inicia tu agente SSH, o usa un archivo de clave o una contraseña.
- Una contraseña o clave rechazada es la respuesta del servidor: revisa **Usuario SSH** y el método.
- Un servidor cuya clave cambió desde la primera conexión se rechaza. Las claves son las de
  `~/.ssh/known_hosts`, compartido con OpenSSH: elimina allí la línea antigua si el cambio es
  esperado.

### Proxy

Los errores nombran el proxy, como `SOCKS5 proxy host:1080: …` o `HTTP proxy host:3128: …`.

- `no answer within 10 s`: el proxy no responde.
- `the proxy asks for a user and password, or refused them`: escríbelos en la URL
  (`http://user:password@host:3128`).
- `cannot reach <host>:<port>: …`: el proxy respondió pero no pudo llegar a la base de datos.
- Una URL que no se entiende se notifica como `proxy "<url>": …`. Un reenvío de puerto va solo:
  quita el túnel SSH o el proxy.

### Llavero

- `keychain error: …`, o "the system has no keychain to save the password in": en Linux debe estar
  en marcha un Secret Service como GNOME Keyring o KWallet. Sin uno, define una contraseña maestra
  (Ajustes › **Contraseña maestra**) o elige **Preguntar cada vez** para la contraseña.
- "No hay ninguna contraseña guardada para esta conexión: se guardará en el llavero." es solo un
  aviso.
- Las contraseñas importadas de otra herramienta sin las suyas (DBeaver con su propia contraseña
  maestra, por ejemplo) se piden en el primer uso.

### Contraseña maestra

- "PlatypusDB está bloqueado" o "Introduce la contraseña maestra para usarlas": escríbela y haz clic
  en **Desbloquear**. El bloqueo por inactividad está en Ajustes › **Seguridad**; las conexiones de
  producción se reconectan al usarlas.
- No se puede recuperar. Si se pierde, las contraseñas guardadas se pierden: vuelve a definir las
  contraseñas en cada conexión.
- Para volver al llavero, Ajustes › **Contraseña maestra** › **Desactivar…** (pide la actual).

## Falla la descarga de un driver

DuckDB, Snowflake, BigQuery, Databricks, Trino, otros drivers ADBC, Oracle e IBM Db2 descargan su
biblioteca la primera vez que se usan. La ventana dice "Descargando el driver de `<name>`…" con un
**Cancelar**, y un fallo muestra un mensaje con **Reintentar**.

- `la descarga falló (comprueba la conexión a Internet)` o `no se puede acceder al registro de
  drivers`: no hay ruta hasta el registro; revisa la red, el proxy o el cortafuegos y luego
  **Reintentar**.
- `the downloaded driver is corrupted (its SHA-256 does not match)`: el archivo se alteró o se cortó
  en tránsito; inténtalo de nuevo, e infórmanos si persiste.
- `no <name> driver for this platform`, o "PlatypusDB descarga DuckDB en Windows, macOS y Linux, con
  procesadores x64 y ARM; no en este sistema.": no hay compilación para tu sistema.
- `the driver's license (…) does not allow downloading it here`: la app no descarga drivers cuya
  licencia lo prohíbe.
- IBM Db2 se alcanza por ODBC solo en Windows por ahora; registrar su driver pide permiso de
  administrador (Control de cuentas de usuario). Responde que sí, o el driver no se registra.
- **Cancelar** deja el formulario utilizable; la oferta vuelve en **Probar la conexión** o en la
  primera apertura.

Ajustes › **Drivers** lista lo que se ha descargado, elimina un driver y muestra los drivers ODBC del
sistema. Una eliminación que Windows rechazó se termina en el siguiente inicio.

## Un agente no ve una conexión

Repasa esta lista en orden.

1. **El acceso está desactivado.** El agente responde "`<name>` is not available to agents". En
   Ajustes › **Agentes** › **Seguridad**, pon la conexión en **Solo lectura** o **Lectura y
   escritura**. Una conexión que viene de un proyecto compartido empieza en **Desactivado** en tu
   ordenador.
2. **Una contraseña maestra.** El agente responde "the saved password of `<name>` is locked by
   PlatypusDB's master password, which agents cannot enter". Dale a esa conexión un comando de
   contraseña o `.pgpass`, o desactiva la contraseña maestra.
3. **La contraseña se pide cada vez.** El agente responde "`<name>` asks for its password at each
   connection, and agents cannot type it". Guárdala en el llavero, o usa `.pgpass` o un comando de
   contraseña.
4. **Escrituras rechazadas.** Las conexiones de producción y de solo lectura nunca pasan de solo
   lectura; en las demás el cambio espera tu respuesta en la app ([Agentes](agents.md)).
5. **El cliente no se reinició** después de **Instalar**, o falta la entrada: Ajustes › **Agentes**
   › **Configuración** dice si PlatypusDB está en el archivo del cliente.
6. **Una copia portátil u otro perfil.** El agente lee las conexiones del perfil desde el que se
   inició; un cliente configurado con la app instalada no ve la carpeta `data` de una copia
   portátil.

## La copia portátil

- Es un ejecutable con una carpeta `data` vacía al lado. Si la carpeta no está al iniciar, la app usa
  en su lugar las carpetas del sistema, así que una carpeta `data` renombrada parece haber perdido las
  conexiones: ponle de nuevo su nombre.
- Todo está en `data`, registros incluidos; la carpeta se mueve con el ejecutable. Las contraseñas
  guardadas se quedan en el llavero del sistema del ordenador, salvo que una contraseña maestra las
  guarde también en la carpeta: en otro ordenador, define antes la contraseña maestra o vuelve a
  introducir las contraseñas.
- No se actualiza sola: sustituye su ejecutable por el nuevo. La pantalla de bienvenida dice
  `portable` junto a la versión.

## Dónde están los datos y los registros

La app guarda las conexiones y los ajustes (`connections.toml`, `settings.toml` y `secrets.vault` con
una contraseña maestra) en su carpeta de configuración, y el historial, los scripts, los drivers
descargados y los registros en su carpeta de datos, ambas llamadas `dev.platypusdb.desktop`:

| Sistema | Carpeta de configuración | Carpeta de datos |
|---|---|---|
| Windows | `%APPDATA%\dev.platypusdb.desktop` | `%APPDATA%\dev.platypusdb.desktop` |
| macOS | `~/Library/Application Support/dev.platypusdb.desktop` | la misma |
| Linux | `~/.config/dev.platypusdb.desktop` | `~/.local/share/dev.platypusdb.desktop` |

Una copia portátil guarda todo en su carpeta `data`, y la variable de entorno `PLATYPUS_DATA_DIR`
pone todo en la carpeta que nombra.

![Ajustes, la página Diagnóstico](../../screenshots/guide/diagnostics.png)

El registro es `logs/platypus.log` en la carpeta de datos: una línea por inicio, error y fallo
grave, rotado a 1 MB con dos archivos anteriores (`platypus.1.log`, `platypus.2.log`). Ajustes ›
**Diagnóstico** muestra la carpeta (**Copiar**) y **Guardar el paquete de diagnóstico…**, un zip con
la versión, el sistema, los ajustes sin contraseñas y los registros, para adjuntar a un informe de
error. No se envía nada a ninguna parte; lee los registros antes de compartirlos, porque un error
puede nombrar un servidor o una tabla. Tras un fallo grave, el siguiente inicio ofrece el mismo
paquete.

## Actualizaciones

- Al iniciar, la app busca una versión más reciente y muestra "PlatypusDB `<version>` está
  disponible" con **Más tarde** e **Instalar y reiniciar**.
- Una copia portátil solo lo avisa: sustituye su ejecutable. Una instalación desde un gestor de
  paquetes (Flatpak, el AUR) lo avisa y deja la actualización en sus manos.
- Los drivers descargados se actualizan aparte: Ajustes › **Drivers** › **Buscar actualizaciones**, o
  los ajustes **Buscar actualizaciones de controladores al iniciar** (activado, como máximo una vez
  al día) e **Instalar las actualizaciones de controladores automáticamente** (desactivado). Los drivers integrados se actualizan
  con la app.

## Preguntas frecuentes

**¿Puedo usar PlatypusDB sin conexión?** Sí, con una base de datos local. Las descargas (drivers,
actualizaciones) necesitan red.

**¿Dónde informo de un error?** En el [gestor de incidencias](https://github.com/platypus-labs/platypus-db/issues),
con el paquete de diagnóstico si puedes.

**¿Dónde están los atajos?** [SHORTCUTS.md](../SHORTCUTS.md), y Ajustes › Atajos de teclado para
cambiarlos.

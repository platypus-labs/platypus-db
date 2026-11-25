<!-- source: 284fda1cc34a -->
[← Guía del usuario](README.md)

# Conectar a una base de datos

![Elegir la base de datos para una conexión nueva](../../../../screenshots/connections.png)

## Desde una URL

1. Haz clic en **Nueva conexión** (o en el `+` del navegador).
2. Pega la URL bajo los logos: `postgres://`, `mysql://`, `sqlserver://`, `clickhouse://`,
   `libsql://`, `mongodb://`, `redis://`, `rediss://`, `elasticsearch://`, `opensearch://`, una
   URL JDBC o una cadena ADO.NET / ODBC. El formulario se abre ya relleno para esa base de datos.
3. Pon la contraseña en el campo **Contraseña**, no en la URL: el formulario te avisa si la URL
   contiene una.
4. Haz clic en **Probar la conexión** y luego en **Guardar**.

O elige la base de datos entre los logos (el buscador de arriba usa `Enter` para la primera
coincidencia). La elección fija el driver; **Atrás** vuelve a los logos.

## El formulario

El formulario tiene cuatro páginas: **General**, **Apariencia y seguridad**, **Red** e **Inicio**.
Un punto junto al nombre de una página indica que contiene ajustes.

- **General**: host, puerto, usuario, base de datos y de dónde viene la contraseña (más abajo).
- **Apariencia y seguridad**: el entorno (Desarrollo, Pruebas o Producción), un color, la casilla
  **Solo lectura (rechaza cambios en los datos)**, las casillas que impiden exportar y editar datos,
  y el acceso de los agentes. Consulta [Seguridad](safety.md) y [Agentes](agents.md).
- **Red**: túnel SSH, modo SSL y certificados, proxy, reenvío de puerto y comandos antes y después de
  la conexión.
- **Inicio**: un archivo de variables, un esquema o una zona horaria por defecto, y sentencias que se
  ejecutan en cada conexión.

## Contraseñas

En **General**, la contraseña viene de una de estas fuentes:

- **Guardada en el llavero**: el llavero del sistema. **Probar la conexión** la envía solo al
  servidor para el que se guardó: si cambias el host, el puerto o los servidores SSH intermedios, te
  pide que escribas la contraseña para esa prueba. Otra base de datos del mismo servidor PostgreSQL
  o MySQL no es otro servidor.
- **Preguntar cada vez**: se escribe en cada conexión. Los agentes no pueden abrir una conexión así.
- **Archivo .pgpass** (solo PostgreSQL; el host, el puerto, la base de datos y el usuario de la
  conexión eligen la línea, como hace libpq).
- **Comando de shell**: un comando que imprime la contraseña.

Cualquier campo puede leer una variable de entorno escrita `${env:NAME}`. Los gestores de contraseñas
(1Password, Bitwarden, HashiCorp Vault, AWS Secrets Manager) están en la misma lista para toda base
de datos cuyo formulario pida una contraseña o un token; ninguno se limita a una sola base de datos.
Indica la referencia del secreto: la app construye con ella el comando de shell (`op read`,
`bw get password`, `vault kv get`, `aws secretsmanager get-secret-value`) y lo ejecuta en cada
conexión. Su herramienta de línea de comandos debe estar instalada y con la sesión iniciada. Nada de
esto se ha probado con una cuenta real de 1Password, Bitwarden, Vault o AWS: solo se prueban los
comandos que construye la app y el propio comando de shell.

Algunas bases de datos inician sesión sin contraseña, y la misma lista las ofrece: un token de la
herramienta de línea de comandos de una nube (AWS IAM y Google Cloud IAM para PostgreSQL y MySQL,
Microsoft Entra ID para esos dos y SQL Server), el inicio de sesión de Windows para SQL Server, y un
**Ticket de Kerberos (GSSAPI)** para PostgreSQL en Windows y Linux. Ejecuta antes `kinit`, o define `KRB5CCNAME` o
`KRB5_CLIENT_KTNAME`; en Windows el ticket es el de tu sesión. El ticket es lo único que llega al
servidor, y una prueba sin él dice qué ejecutar. La [página de PostgreSQL](databases/postgresql.md)
tiene los detalles, y qué está probado y qué no: un ticket real ha iniciado sesión desde Linux, aún
no a través de SSPI de Windows.

### Contraseña maestra

Una contraseña maestra guarda las contraseñas en un archivo cifrado en lugar del llavero.

1. Abre Ajustes (`Ctrl+,`) › **Contraseña maestra**.
2. Haz clic en **Establecer una contraseña maestra…**, escríbela dos veces (8 caracteres como
   mínimo) y confirma.
3. Para volver al llavero, haz clic en **Desactivar…** en la misma página. **Bloquear ahora** bloquea
   al instante.

No se puede recuperar: sin ella, las contraseñas guardadas se pierden. Los agentes no pueden
introducirla (consulta
[Solución de problemas](troubleshooting.md#un-agente-no-ve-una-conexión)).

## TLS

En la página **Red**, **Modo SSL** admite los valores de libpq: `disable`, `prefer`, `require`,
`verify-ca` y `verify-full`. `prefer` y `require` cifran sin comprobar el certificado;
`verify-ca` y `verify-full` lo comprueban, así que puede hacer falta un **Certificado CA (PEM)**.
Para los servidores que autentican a los clientes con certificado, indica el **Certificado de
cliente (PEM)** y la **Clave de cliente (PEM)** (PostgreSQL y MySQL).

## Túnel SSH

![Un túnel SSH en la página Red](../../../../screenshots/guide/connect-network.png)

1. En la página **Red**, marca **Conectar a través de un túnel SSH**.
2. Rellena **Host SSH**, el puerto y **Usuario SSH**.
3. Autentícate con una contraseña, un **Archivo de clave privada** (y su frase de paso) o el
   **Agente SSH**. Para un bastión, añade un **Jump host** (**Primero a través de un jump host**).
4. Haz clic en **Probar la conexión**.

La clave del servidor se comprueba contra `~/.ssh/known_hosts`: un servidor desconocido se aprende
la primera vez, y una clave que ha cambiado desde entonces se rechaza. Un **Perfil de red**
comparte un túnel y una configuración SSL entre conexiones.

## Proxy y reenvío de puerto

También en **Red**: un proxy SOCKS5 o HTTP (`socks5://user:password@host:1080`,
`http://host:3128`), o un reenvío de puerto mediante `kubectl` o una sesión de AWS Systems Manager (la
CLI debe estar instalada y con la sesión iniciada). Un reenvío de puerto va solo: quita el túnel SSH
o el proxy.

## Siguiente paso

Los detalles de cada base de datos: [Bases de datos](databases/README.md). El inicio de sesión en la
nube, los poolers, las importaciones desde DBeaver, DataGrip y pgAdmin (conexiones de Db2 incluidas)
y la exportación de conexiones están en [FEATURES.md](../../../FEATURES.md#connections). Si no conecta,
consulta [Solución de problemas](troubleshooting.md#errores-de-conexión).

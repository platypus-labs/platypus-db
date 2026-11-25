<!-- source: 7b39ba524d58 -->
[← Bases de datos](README.md)

# libSQL y Turso

El driver habla el protocolo **Hrana 3** sobre HTTP(S) con un servidor libSQL (`sqld`) o con Turso. El
SQL es el de SQLite, así que el navegador, los tipos y los planes también son los de SQLite. El driver
es parte de la app; no se descarga nada.

## Conectar

| Pegas | Se interpreta como |
|---|---|
| `libsql://db-acme.turso.io?authToken=…` | HTTPS a ese host, con el token como contraseña |
| `libsql://:token@localhost:8080?sslmode=disable` | HTTP sin cifrar, con el token en la posición de la contraseña |
| `libsql://localhost:8080?tls=0&authToken=t` | `tls=0` es HTTP sin cifrar, `tls=1` es HTTPS |
| `wss://…`, `ws://…` | el mismo servidor sobre HTTP(S): `wss` es TLS, `ws` no |
| `https://host` o `http://host` en un formulario de libSQL | la dirección HTTP del servidor |

Sin puerto se usa el del esquema: 443, o 80 cuando TLS está desactivado.

- **Token**: el campo de contraseña, o `authToken` en la URL.
- **TLS**: una URL `libsql://` es HTTPS con el certificado y el nombre del host comprobados
  (`verify-full`), porque la URL lleva el token. El icono de Turso del formulario también empieza en
  `verify-full`. Para un `sqld` local sin TLS elige `disable`, o usa `tls=0` en la URL. `sslrootcert`
  nombra el archivo de tu CA; si no, se usan los certificados de confianza del sistema.

## Privilegios

Lo que permita el token: la app envía las sentencias que ejecutas, y las consultas del catálogo de
SQLite para el navegador.

## Qué funciona

- Edición de datos en la cuadrícula, por clave primaria (o una clave única en columnas no nulas, o una
  clave virtual).
- Confirmación manual y rollback. Una sentencia se ejecuta en un stream que el servidor mantiene
  abierto entre peticiones (un `baton`), así que una transacción y las tablas temporales duran. Si el
  servidor cierra el stream fuera de una transacción, la app abre otro y continúa; dentro de una
  transacción, la conexión se da por perdida y la transacción con ella.
- Explain muestra el plan de consulta de SQLite; no hay Explain analyze.
- Búsqueda de metadatos y DDL de objetos, y herramientas de esquema como en SQLite.
- Sin pestañas de sesiones, bloqueos, panel, usuarios ni parámetros.

## Qué no funciona

- **Cancelar**: HTTP no tiene forma de detener una sentencia en ejecución, así que Cancelar responde
  que cancelar sentencias libSQL no está soportado.
- Los resultados llegan enteros desde el servidor, no fila a fila.
- El modo terminal no está disponible: los servidores libSQL no tienen un cliente de línea de comandos
  con el que ejecutar scripts.

## Agentes

Los agentes de solo lectura y las conexiones de solo lectura dependen solo de la comprobación de la
sentencia; no se le dice al servidor que rechace las escrituras. Consulta
[Agentes y la CLI](../../AGENTS.md).

## Límites conocidos

- Un fallo de red después de enviar una sentencia puede dejar sin saber si se ejecutó; la app no la
  vuelve a enviar.

## Probado en

La batería de pruebas de conformidad se ejecuta en `sqld` v0.24.33 (contenedor
`ghcr.io/tursodatabase/libsql-server`), sin TLS. La nube de Turso no está probada.

Consulta también: [Conectar](../connect.md), [Solución de problemas](../troubleshooting.md).

<!-- source: aa482a6e3843 -->
[← Guía del usuario](README.md)

# Proyectos y uso compartido

Un proyecto es un grupo de conexiones, scripts y pestañas abiertas, que se muestra en la barra de la
izquierda. Los detalles están en [FEATURES.md](../../../FEATURES.md#workspace) y
[FEATURES.md](../../../FEATURES.md#sharing-and-sync).

## Crear un proyecto

1. Ejecuta **Nuevo proyecto…** desde la paleta de comandos (`Ctrl+K`).
2. Ponle un nombre y elige las conexiones que mostrará. También se puede añadir una conexión desde
   su propio formulario: **Proyectos**, al final de la página **Apariencia y seguridad**.
3. `Ctrl+1` muestra todas las conexiones, y de `Ctrl+2` a `Ctrl+9` los proyectos en el orden de la
   barra.

## Compartir una consulta

1. En una pestaña de editor SQL, ejecuta **Compartir consulta…** (menú de la pestaña o paleta de
   comandos).
2. Copia el enlace `platypus://…`, o guarda un archivo `.platypus`. Contiene el SQL, su título y, si
   lo pides, los valores de los parámetros; nombra la base de datos pero nunca una contraseña.
3. Tu colega lo abre con **Abrir un enlace compartido…** o **Abrir una consulta compartida…**, o
   soltando el archivo en la ventana. La app busca la conexión guardada que corresponde o ofrece una
   nueva ya rellena, y muestra la consulta sin ejecutarla.

## Compartir un proyecto con un equipo

1. Haz clic derecho en el proyecto de la barra y elige **Compartir proyecto…**, y elige una carpeta
   que el equipo mantenga en Git o en una unidad compartida. Contiene las conexiones (sin
   credenciales) y los scripts.
2. Cada colega hace clic en **Abrir proyecto compartido…** en la pantalla de bienvenida, elige la
   carpeta e introduce sus propias contraseñas.
3. Tras un cambio en la carpeta, ejecuta **Volver a leer la carpeta compartida** desde el menú del
   proyecto.

Seguro por diseño: una conexión nueva de un proyecto compartido da a los agentes **ningún acceso** en
tu ordenador hasta que decidas otra cosa, y las sentencias que el archivo ejecuta en cada conexión
se muestran antes y solo se ejecutan cuando apruebas ese texto exacto.

## Llevar las conexiones a otro ordenador

En la pantalla de bienvenida, **Exportar conexiones** escribe un archivo `.platypusconn`: elige las
conexiones y, si quieres, una contraseña de 8 caracteres como mínimo, que cifra el archivo y le
permite llevar las contraseñas guardadas. En el otro ordenador, **Importar conexiones** lo lee. Sin
contraseña, el archivo no contiene ninguna credencial.

Para mantener ajustes, conexiones y scripts sincronizados entre tus ordenadores, usa Ajustes ›
**Sincronización** con una carpeta en una unidad en la nube. Consulta
[FEATURES.md](../../../FEATURES.md#sharing-and-sync).

## Scripts y Git

Cuando la carpeta de scripts de un proyecto es un repositorio Git, el panel Scripts muestra la rama
y los cambios de cada script, y ofrece **Commit…**, **Pull** y **Push**. Git debe estar instalado.

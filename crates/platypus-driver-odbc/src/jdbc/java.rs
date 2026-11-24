//! Finding Java, and the bridge program the app starts with it.

use std::path::{Path, PathBuf};

use platypus_core::{Error, Result};

/// The bridge program, built from `java/Bridge.java` by `scripts/jdbc-bridge.mjs`.
const BRIDGE_JAR: &[u8] = include_bytes!("../../java/platypus-jdbc-bridge.jar");

fn executable() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

/// The `java` to run: the one the connection names (a program, or a Java home), else
/// `JAVA_HOME`, the `PATH`, and DBeaver's own runtime, which a machine with DBeaver has.
pub fn find(own: Option<&str>) -> Result<PathBuf> {
    if let Some(own) = own.map(str::trim).filter(|p| !p.is_empty()) {
        let path = PathBuf::from(own);
        let program = if path.is_dir() {
            path.join("bin").join(executable())
        } else {
            path
        };
        return if program.is_file() {
            Ok(program)
        } else {
            Err(Error::Config(format!(
                "the Java named in the connection does not exist: {}",
                program.display()
            )))
        };
    }
    candidates()
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| {
            Error::Config(
            "this connection needs Java 11 or later: install a JRE, set JAVA_HOME, or name the \
             `java` program (or its folder) in the connection's JDBC settings"
                .to_owned(),
        )
        })
}

fn candidates() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        found.push(PathBuf::from(home).join("bin").join(executable()));
    }
    if let Some(path) = std::env::var_os("PATH") {
        found.extend(std::env::split_paths(&path).map(|dir| dir.join(executable())));
    }
    for var in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            found.push(
                Path::new(&base)
                    .join("DBeaver")
                    .join("jre")
                    .join("bin")
                    .join(executable()),
            );
        }
    }
    found
}

/// The bridge's jar, written to the temporary folder when it is not there as it should be.
pub fn bridge_jar() -> Result<PathBuf> {
    use std::hash::{DefaultHasher, Hash as _, Hasher as _};
    let mut hasher = DefaultHasher::new();
    BRIDGE_JAR.hash(&mut hasher);
    let path =
        std::env::temp_dir().join(format!("platypus-jdbc-bridge-{:016x}.jar", hasher.finish()));
    // It is code the app runs: whatever is there is replaced unless it is exactly the jar.
    if std::fs::read(&path).is_ok_and(|on_disk| on_disk == BRIDGE_JAR) {
        return Ok(path);
    }
    let write = || -> std::io::Result<()> {
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&tmp, BRIDGE_JAR)?;
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&tmp, &path)
    };
    write().map_err(|e| {
        Error::Connect(format!(
            "cannot write the JDBC bridge to {}: {e}",
            path.display()
        ))
    })?;
    Ok(path)
}

/// The class path of the bridge's JVM: the bridge, then the driver's jars.
pub fn class_path(bridge: &Path, jars: &[&str]) -> String {
    let separator = if cfg!(windows) { ";" } else { ":" };
    std::iter::once(bridge.to_string_lossy().into_owned())
        .chain(jars.iter().map(|j| (*j).to_owned()))
        .collect::<Vec<_>>()
        .join(separator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_class_path_starts_with_the_bridge() {
        let path = class_path(Path::new("b.jar"), &["a.jar", "l.jar"]);
        let separator = if cfg!(windows) { ';' } else { ':' };
        assert_eq!(path, format!("b.jar{separator}a.jar{separator}l.jar"));
    }

    #[test]
    fn a_java_that_is_not_there_is_an_error_that_names_it() {
        let e = find(Some("/no/such/java")).unwrap_err().to_string();
        assert!(e.contains("does not exist"), "{e}");
    }

    #[test]
    fn the_embedded_jar_is_a_zip() {
        assert_eq!(&BRIDGE_JAR[..2], b"PK");
    }
}

//! Opens a saved connection the way the app does: the network profile's tunnel and SSL in
//! place of the connection's own, the password from its command, `.pgpass` or the
//! keychain, the SSH secrets, and the bootstrap statements run after connecting.

use platypus_core::{Connection, ConnectionConfig, Driver, DriverKind, Error, ExecOptions, Secret};
use platypus_driver_adbc::AdbcDriver;
use platypus_driver_clickhouse::ClickHouseDriver;
use platypus_driver_duckdb::DuckDbDriver;
use platypus_driver_libsql::LibsqlDriver;
use platypus_driver_mongo::MongoDriver;
use platypus_driver_mssql::MssqlDriver;
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;
use platypus_sql::Dialect;
use platypus_store::{ConnectionStore, SavedConnection, SavedSsh, SshAuthKind, StoreError};
use uuid::Uuid;

use crate::error::{AgentError, Result};
use crate::redact::Redactor;

/// How sessions opened for agents introduce themselves to the server.
const APPLICATION_NAME: &str = "PlatypusDB agent";

/// Opens a connection through its SSH tunnel when it has one; `config` is ready to use.
pub async fn connect(config: &ConnectionConfig) -> Result<Box<dyn Connection>, Error> {
    platypus_net::connect(config, None, |config| async move {
        match config.driver {
            DriverKind::Postgres => PgDriver.connect(&config).await,
            DriverKind::MySql => MySqlDriver.connect(&config).await,
            DriverKind::Sqlite => SqliteDriver.connect(&config).await,
            DriverKind::DuckDb => DuckDbDriver.connect(&config).await,
            DriverKind::SqlServer => MssqlDriver.connect(&config).await,
            DriverKind::Libsql => LibsqlDriver.connect(&config).await,
            DriverKind::ClickHouse => ClickHouseDriver.connect(&config).await,
            DriverKind::Mongo => MongoDriver.connect(&config).await,
            DriverKind::Snowflake
            | DriverKind::BigQuery
            | DriverKind::Databricks
            | DriverKind::Trino
            | DriverKind::Adbc => AdbcDriver::shared()?.connect(&config).await,
        }
    })
    .await
}

/// A store error about `name`'s secrets, with the locked vault said plainly.
fn secret_error(name: &str, e: StoreError) -> AgentError {
    match e {
        StoreError::Locked => AgentError::Locked(name.to_owned()),
        other => AgentError::Store(other.to_string()),
    }
}

/// The connection's password, wherever it comes from.
async fn password(store: &ConnectionStore, saved: &SavedConnection) -> Result<Option<Secret>> {
    if saved.driver.is_embedded() {
        return Ok(None);
    }
    if saved.password_command.is_some() || saved.pgpass {
        let lookup = saved.clone();
        // A password command may take a while: it runs off the async runtime.
        return tokio::task::spawn_blocking(move || platypus_store::external_password(&lookup))
            .await
            .map_err(|e| AgentError::Store(e.to_string()))?
            .map_err(|e| secret_error(&saved.name, e));
    }
    if saved.asks_password() {
        return Err(AgentError::Denied(format!(
            "`{}` asks for its password at each connection, and agents cannot type it: save it \
             in the keychain, or use .pgpass or a password command",
            saved.name
        )));
    }
    store
        .password(saved.id)
        .map_err(|e| secret_error(&saved.name, e))
}

/// The secrets of the SSH servers that use one: a password always, a passphrase when the
/// key has one (with the vault locked, the key is tried without).
fn ssh_secrets(
    store: &ConnectionStore,
    ssh: &SavedSsh,
    name: &str,
) -> Result<Vec<(Uuid, Option<Secret>)>> {
    ssh.chain()
        .map(|host| {
            let secret = match (host.auth, store.ssh_secret(host.secret_id)) {
                (_, Ok(secret)) => secret,
                (SshAuthKind::Key | SshAuthKind::Agent, Err(StoreError::Locked)) => None,
                (_, Err(e)) => return Err(secret_error(name, e)),
            };
            Ok((host.secret_id, secret))
        })
        .collect()
}

/// The settings to open `saved` with, and the secrets they hold for the redactor.
pub async fn config_of(
    store: &ConnectionStore,
    saved: &SavedConnection,
) -> Result<(ConnectionConfig, Redactor)> {
    let effective = store.effective(saved);
    let mut redactor = Redactor::default();
    if let Some(command) = &effective.password_command {
        redactor.add(command);
    }
    // A password command's error may echo what it printed.
    let password = password(store, &effective).await.map_err(|e| match e {
        AgentError::Store(message) => AgentError::Store(redactor.apply(&message)),
        other => other,
    })?;
    redactor.add_secret(password.as_ref());
    let mut config = effective.config(password);
    if let Some(ssh) = &effective.ssh {
        let secrets = ssh_secrets(store, ssh, &effective.name)?;
        for (_, secret) in &secrets {
            redactor.add_secret(secret.as_ref());
        }
        config.ssh = Some(ssh.config(&|id| {
            secrets
                .iter()
                .find(|(secret_id, _)| *secret_id == id)
                .and_then(|(_, secret)| secret.clone())
        }));
    }
    let script = &effective.init.statements;
    config.init.statements = platypus_sql::split(script, Dialect::from(effective.driver))
        .iter()
        .map(|s| s.text(script).to_owned())
        .collect();
    config.application_name = Some(APPLICATION_NAME.to_owned());
    Ok((config, redactor))
}

/// What a session opened for agents runs first. DuckDB reads and writes local files and
/// loads extensions from SQL (`read_text`, `glob`, `COPY`, `INSTALL`): agents get none of it
/// but the data file the connection reads, and cannot turn it back on.
fn agent_session_setup(config: &ConnectionConfig) -> Vec<String> {
    if config.driver != DriverKind::DuckDb {
        return Vec::new();
    }
    let mut setup = Vec::new();
    if platypus_driver_duckdb::files::is_data_file(&config.database) {
        setup.push(format!(
            "SET allowed_paths = [{}]",
            Dialect::DuckDb.quote_literal(&config.database)
        ));
    }
    setup.extend(
        [
            "SET enable_external_access = false",
            "SET autoinstall_known_extensions = false",
            "SET autoload_known_extensions = false",
            "SET lock_configuration = true",
        ]
        .map(str::to_owned),
    );
    setup
}

/// Opens `saved` for agents; errors are redacted.
pub async fn open(
    store: &ConnectionStore,
    saved: &SavedConnection,
) -> Result<(Box<dyn Connection>, Redactor)> {
    let (config, redactor) = config_of(store, saved).await?;
    let failed = |e: Error| AgentError::Connect {
        name: saved.name.clone(),
        message: redactor.apply(&e.to_string()),
    };
    let conn = connect(&config).await.map_err(failed)?;
    for sql in agent_session_setup(&config) {
        conn.execute(&sql, &ExecOptions::default())
            .await
            .map_err(failed)?;
    }
    Ok((conn, redactor))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use platypus_store::{MemorySecrets, PasswordUpdate, SecretStore};

    use super::*;

    fn store(dir: &std::path::Path, secrets: Arc<dyn SecretStore>) -> ConnectionStore {
        ConnectionStore::open(dir.join("connections.toml"), secrets).unwrap()
    }

    #[tokio::test]
    async fn uses_the_saved_password_and_ssh_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(dir.path(), Arc::new(MemorySecrets::default()));
        let mut saved = SavedConnection::new("PG", DriverKind::Postgres);
        saved.init.statements = "SET a = 1; SET b = 2".to_owned();
        let ssh = SavedSsh {
            host: "bastion".into(),
            port: 22,
            user: "ops".into(),
            auth: SshAuthKind::Password,
            key_path: None,
            secret_id: Uuid::new_v4(),
            jump: None,
        };
        saved.ssh = Some(ssh.clone());
        s.save(saved.clone(), PasswordUpdate::Set(Secret::new("db-secret")))
            .unwrap();
        s.set_ssh_secret(ssh.secret_id, Some(&Secret::new("ssh-secret")))
            .unwrap();

        let (config, redactor) = config_of(&s, &saved).await.unwrap();
        assert_eq!(config.password, Some(Secret::new("db-secret")));
        assert_eq!(
            config.ssh.unwrap().auth,
            platypus_core::SshAuth::Password(Secret::new("ssh-secret"))
        );
        assert_eq!(config.init.statements, ["SET a = 1", "SET b = 2"]);
        assert_eq!(config.application_name.as_deref(), Some(APPLICATION_NAME));
        assert_eq!(
            redactor.apply("bad db-secret / ssh-secret"),
            "bad *** / ***"
        );
    }

    /// A DuckDB connection saved for `database`, opened for agents.
    async fn duckdb_for_agents(
        dir: &std::path::Path,
        database: &std::path::Path,
    ) -> Box<dyn Connection> {
        let s = store(dir, Arc::new(MemorySecrets::default()));
        let mut saved = SavedConnection::new("Duck", DriverKind::DuckDb);
        saved.database = database.display().to_string();
        open(&s, &saved).await.unwrap().0
    }

    async fn run(conn: &dyn Connection, sql: &str) -> Result<(), Error> {
        use futures::TryStreamExt;
        if let platypus_core::ExecResult::Rows(rows) =
            conn.execute(sql, &ExecOptions::default()).await?
        {
            let _: Vec<_> = rows.try_collect().await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn duckdb_sessions_of_agents_reach_no_files() {
        let dir = tempfile::tempdir().unwrap();
        let secret = dir.path().join("secret.txt");
        std::fs::write(&secret, "s3cret").unwrap();
        let csv = dir.path().join("people.csv");
        std::fs::write(
            &csv,
            "id,name
1,Ada
",
        )
        .unwrap();
        let read_secret = format!("SELECT * FROM read_text('{}')", secret.display());

        let conn = duckdb_for_agents(dir.path(), &dir.path().join("agent.duckdb")).await;
        run(&*conn, "SELECT 42").await.unwrap();
        for sql in [
            read_secret.as_str(),
            &format!("SELECT * FROM glob('{}/*')", dir.path().display()),
            "SET enable_external_access = true",
            "INSTALL httpfs",
        ] {
            assert!(run(&*conn, sql).await.is_err(), "{sql}");
        }

        // A data file reads itself, and nothing else.
        let conn = duckdb_for_agents(dir.path(), &csv).await;
        run(&*conn, "SELECT * FROM people").await.unwrap();
        assert!(run(&*conn, &read_secret).await.is_err());
    }

    #[tokio::test]
    async fn a_locked_vault_is_reported_and_typed_passwords_refused() {
        let dir = tempfile::tempdir().unwrap();
        let vault = platypus_store::VaultSecrets::locked(dir.path().join("secrets.vault"));
        let mut s = store(dir.path(), Arc::new(vault));
        let saved = SavedConnection::new("Locked PG", DriverKind::Postgres);
        s.save(saved.clone(), PasswordUpdate::Keep).unwrap();
        let err = config_of(&s, &saved).await.unwrap_err();
        assert!(matches!(err, AgentError::Locked(_)), "{err}");
        assert!(err.to_string().contains("master password"), "{err}");

        let mut asks = saved.clone();
        asks.save_password = false;
        let err = config_of(&s, &asks).await.unwrap_err();
        assert!(matches!(err, AgentError::Denied(_)), "{err}");

        let mut command = asks.clone();
        command.password_command = Some("echo from-command".to_owned());
        let (config, redactor) = config_of(&s, &command).await.unwrap();
        assert_eq!(config.password, Some(Secret::new("from-command")));
        assert_eq!(redactor.apply("echo from-command"), "***");
    }
}

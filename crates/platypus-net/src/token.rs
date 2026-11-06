//! Passwords that are tokens from a cloud's CLI, made at each connection: AWS IAM
//! (`aws rds generate-db-auth-token`), Cloud SQL IAM (`gcloud sql generate-login-token`) and
//! Microsoft Entra ID (`az account get-access-token`). Tokens last minutes, so they are never
//! saved: a reconnection asks for a new one.

use std::time::Duration;

use platypus_core::{Auth, ConnectionConfig, DriverKind, Error, Result, Secret};

use crate::cli;

/// How long the CLI may take: signing in again can open a browser.
const TOKEN_TIMEOUT: Duration = Duration::from_secs(120);

/// The CLI and its arguments that print `config`'s token, `None` when its password is not a
/// token.
pub(crate) fn command(config: &ConnectionConfig) -> Result<Option<(&'static str, Vec<String>)>> {
    let args = |items: &[&str]| items.iter().map(|&s| s.to_owned()).collect::<Vec<_>>();
    Ok(Some(match config.auth()? {
        Auth::Password | Auth::Windows | Auth::Kerberos => return Ok(None),
        Auth::AwsIam => {
            let port = config
                .port
                .or(config.driver.default_port())
                .ok_or_else(|| {
                    Error::Config("IAM authentication needs a server port".to_owned())
                })?;
            let mut args = args(&[
                "rds",
                "generate-db-auth-token",
                "--hostname",
                cli::argument("the host", &config.host)?,
                "--port",
                &port.to_string(),
                "--username",
                cli::argument("the user name", &config.user)?,
            ]);
            args.extend(cli::aws_flags(config)?);
            ("aws", args)
        }
        Auth::GcpIam => ("gcloud", args(&["sql", "generate-login-token"])),
        Auth::EntraId => {
            // SQL Server's resource, or the one of Azure's PostgreSQL and MySQL.
            let resource = if config.driver == DriverKind::SqlServer {
                "https://database.windows.net/"
            } else {
                "https://ossrdbms-aad.database.windows.net"
            };
            (
                "az",
                args(&[
                    "account",
                    "get-access-token",
                    "--resource",
                    resource,
                    "--query",
                    "accessToken",
                    "--output",
                    "tsv",
                ]),
            )
        }
    }))
}

/// `config`'s token, made by its CLI, or `None` when its password is not a token.
pub(crate) async fn fetch(config: &ConnectionConfig) -> Result<Option<Secret>> {
    let Some((program, args)) = command(config)? else {
        return Ok(None);
    };
    let printed = cli::output(program, &args, TOKEN_TIMEOUT).await?;
    match printed.lines().map(str::trim).find(|l| !l.is_empty()) {
        Some(token) => Ok(Some(Secret::new(token))),
        None => Err(cli::failed(program, "printed no token")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_core::{AUTH, AWS_REGION};

    fn config(driver: DriverKind, auth: &str) -> ConnectionConfig {
        let mut config = ConnectionConfig::new(driver);
        config.host = "db.abc.eu-west-1.rds.amazonaws.com".to_owned();
        config.user = "app".to_owned();
        config.options.insert(AUTH.to_owned(), auth.to_owned());
        config
    }

    #[test]
    fn aws_tokens_are_for_the_host_port_and_user() {
        let mut config = config(DriverKind::Postgres, "aws_iam");
        config
            .options
            .insert(AWS_REGION.to_owned(), "eu-west-1".to_owned());
        let (program, args) = command(&config).unwrap().unwrap();
        assert_eq!(program, "aws");
        assert_eq!(
            args.join(" "),
            "rds generate-db-auth-token --hostname db.abc.eu-west-1.rds.amazonaws.com --port 5432 --username app --region eu-west-1"
        );
    }

    #[test]
    fn values_that_would_be_flags_are_refused() {
        let aws = || config(DriverKind::Postgres, "aws_iam");
        let mut flagged = aws();
        flagged.user = "--debug".to_owned();
        let error = command(&flagged).unwrap_err().to_string();
        assert!(error.contains("user name"), "{error}");
        let mut flagged = aws();
        flagged.options.insert(
            AWS_REGION.to_owned(),
            "--endpoint-url=http://evil".to_owned(),
        );
        assert!(command(&flagged).is_err());
        let mut flagged = aws();
        flagged.host = "-h".to_owned();
        assert!(command(&flagged).is_err());
    }

    #[test]
    fn entra_id_asks_for_the_resource_of_the_database() {
        let sql_server = command(&config(DriverKind::SqlServer, "entra_id"))
            .unwrap()
            .unwrap();
        assert!(
            sql_server
                .1
                .contains(&"https://database.windows.net/".to_owned())
        );
        let postgres = command(&config(DriverKind::Postgres, "entra_id"))
            .unwrap()
            .unwrap();
        assert_eq!(postgres.0, "az");
        assert!(
            postgres
                .1
                .contains(&"https://ossrdbms-aad.database.windows.net".to_owned())
        );
    }

    #[test]
    fn passwords_windows_and_kerberos_sign_in_need_no_token() {
        assert!(
            command(&config(DriverKind::Postgres, ""))
                .unwrap()
                .is_none()
        );
        assert!(
            command(&config(DriverKind::SqlServer, "windows"))
                .unwrap()
                .is_none()
        );
        assert!(
            command(&config(DriverKind::Postgres, "kerberos"))
                .unwrap()
                .is_none()
        );
        assert!(command(&config(DriverKind::Postgres, "ntlm")).is_err());
        let gcp = command(&config(DriverKind::MySql, "gcp_iam"))
            .unwrap()
            .unwrap();
        assert_eq!(
            (gcp.0, gcp.1.join(" ").as_str()),
            ("gcloud", "sql generate-login-token")
        );
    }

    #[tokio::test]
    async fn a_way_to_sign_in_must_fit_the_database() {
        let mut windows_on_pg = config(DriverKind::Postgres, "windows");
        let error = crate::sign_in(&mut windows_on_pg)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Windows sign-in does not sign in to PostgreSQL"),
            "{error}"
        );
        let mut aws_on_sql_server = config(DriverKind::SqlServer, "aws_iam");
        assert!(crate::sign_in(&mut aws_on_sql_server).await.is_err());
        let mut kerberos_on_mysql = config(DriverKind::MySql, "kerberos");
        let error = crate::sign_in(&mut kerberos_on_mysql)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Kerberos does not sign in to"), "{error}");
        let mut kerberos = config(DriverKind::Postgres, "kerberos");
        crate::sign_in(&mut kerberos).await.unwrap();
        assert!(kerberos.password.is_none());
        let mut password = config(DriverKind::SqlServer, "");
        crate::sign_in(&mut password).await.unwrap();
        assert!(password.password.is_none());
    }

    #[tokio::test]
    async fn a_missing_cli_says_so() {
        let error = cli::output("platypus-no-such-cli", &[], TOKEN_TIMEOUT)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("platypus-no-such-cli"), "{error}");
        assert!(error.contains("PATH"), "{error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_cli_prints_the_token_or_says_why_not() {
        let args = |script: &str| vec!["-c".to_owned(), script.to_owned()];
        let printed = cli::output("sh", &args("echo tok-123"), TOKEN_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(printed.trim(), "tok-123");
        let error = cli::output(
            "sh",
            &args("echo 'not signed in' >&2; exit 2"),
            TOKEN_TIMEOUT,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("not signed in"), "{error}");
    }
}

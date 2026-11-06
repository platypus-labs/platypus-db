//! The Kerberos login: which ticket to ask for, and what to tell the user when it fails. The
//! handshake is in `vendor/tokio-postgres/src/gss.rs`, behind the `kerberos` feature.

use platypus_core::{Auth, ConnectionConfig, Error, Result};
use tokio_postgres::GssError;

/// Whether this build has the login. Without it, a connection that asks for Kerberos fails and
/// says so; nothing else changes.
pub const AVAILABLE: bool = cfg!(feature = "kerberos");

/// The service principal `config` logs in to, `None` when it does not use Kerberos. The name is
/// `service/host`, in the realm the system's Kerberos configuration gives. The host goes in
/// lower case, as `kinit` and libpq write it, and is not looked up: through a tunnel it is still
/// the server's name.
pub(crate) fn target(config: &ConnectionConfig) -> Result<Option<String>> {
    if !matches!(config.auth()?, Auth::Kerberos) {
        return Ok(None);
    }
    let service = config.krb_service();
    if !service
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(Error::Config(format!(
            "the Kerberos service name `{service}` is not valid: it is a single word, usually `postgres`"
        )));
    }
    let host = config.host.trim();
    if host.is_empty() || host.starts_with('/') {
        return Err(Error::Config(
            "a Kerberos login needs the server's name as the host, the one in its service \
             principal: not an empty host or a socket path"
                .to_owned(),
        ));
    }
    Ok(Some(format!("{service}/{}", host.to_ascii_lowercase())))
}

/// What to tell the user about `error`, when Kerberos is why it failed.
pub(crate) fn explain(error: &tokio_postgres::Error) -> Option<String> {
    let gss = std::error::Error::source(error)?.downcast_ref::<GssError>()?;
    Some(describe(gss, cfg!(windows)))
}

fn describe(error: &GssError, windows: bool) -> String {
    match error {
        GssError::NotBuilt => "This build of PlatypusDB has no Kerberos: use a build with the \
             `kerberos` feature, or choose another way to sign in."
            .to_owned(),
        GssError::NotChosen => "The server asks for Kerberos (`gss` in `pg_hba.conf`). Choose \
             Kerberos under Password from, in the connection's settings."
            .to_owned(),
        GssError::PasswordAsked => "The server asks this user for a password, and this \
             connection signs in with Kerberos, so none was sent. Check that `pg_hba.conf` has a \
             `gss` line for this user and address, or choose another way to sign in."
            .to_owned(),
        GssError::NotAsked => "The server let this user in without asking for Kerberos, so it \
             has not proved who it is. Check that `pg_hba.conf` has a `gss` line for this user \
             and address, or choose another way to sign in."
            .to_owned(),
        GssError::Unproven => "The server said the login is done without proving who it is: \
             this is not the Kerberos service it should be."
            .to_owned(),
        GssError::Failed { target, detail } if windows => format!(
            "Windows has no Kerberos ticket for `{target}`. Sign in to the domain of the server \
             (`klist` lists your tickets), and check the service name and the host, which must \
             be the name in the service's principal. Windows said: {}",
            detail.trim_start_matches("ClientCtx::step failed ").trim()
        ),
        GssError::Failed { target, detail } => failed(target, detail),
        other => other.to_string(),
    }
}

/// What the system's Kerberos said, in English on the systems that translate nothing of it, with
/// what to do. The raw text comes last: it is what an administrator looks for.
fn failed(target: &str, detail: &str) -> String {
    let text = detail.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    let advice = if has(&["expired"]) {
        "The Kerberos ticket has expired. Run `kinit` to get a new one, then connect again."
            .to_owned()
    } else if has(&[
        "server not found",
        "unknown server",
        "server principal unknown",
    ]) {
        format!(
            "The Kerberos server does not know the service `{target}`. Check the service name \
             (usually `postgres`), and that the host is the name in the service's principal, \
             not an IP address or an alias."
        )
    } else if has(&[
        "cannot contact",
        "cannot find kdc",
        "can't contact",
        "resolve",
    ]) {
        "The Kerberos server (KDC) cannot be reached. Check the realm in `krb5.conf` (or \
         `KRB5_CONFIG`) and the network."
            .to_owned()
    } else if has(&["default realm"]) {
        "Kerberos has no default realm. Set `default_realm` in `krb5.conf` (or point \
         `KRB5_CONFIG` to the file)."
            .to_owned()
    } else {
        "No usable Kerberos ticket was found. Run `kinit <principal>` first, or set `KRB5CCNAME` \
         to the ticket cache, or `KRB5_CLIENT_KTNAME` to a keytab; `klist` shows the tickets you \
         have."
            .to_owned()
    };
    format!("{advice} Kerberos said: {}", detail.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_core::{AUTH, DriverKind, KRB_SERVICE};

    fn kerberos(host: &str) -> ConnectionConfig {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.host = host.to_owned();
        config
            .options
            .insert(AUTH.to_owned(), "kerberos".to_owned());
        config
    }

    #[test]
    fn the_target_is_the_service_on_the_host() {
        let mut config = kerberos("DB.Corp.Example");
        assert_eq!(
            target(&config).unwrap().as_deref(),
            Some("postgres/db.corp.example")
        );
        config
            .options
            .insert(KRB_SERVICE.to_owned(), " pg-prod ".to_owned());
        assert_eq!(
            target(&config).unwrap().as_deref(),
            Some("pg-prod/db.corp.example")
        );
        config
            .options
            .insert(KRB_SERVICE.to_owned(), "   ".to_owned());
        assert_eq!(
            target(&config).unwrap().as_deref(),
            Some("postgres/db.corp.example"),
            "a blank service name is the default"
        );
    }

    #[test]
    fn a_service_name_cannot_change_the_principal() {
        for service in ["a/b", "pg@OTHER.REALM", "two words"] {
            let mut config = kerberos("db");
            config
                .options
                .insert(KRB_SERVICE.to_owned(), service.to_owned());
            let error = target(&config).unwrap_err().to_string();
            assert!(error.contains("service name"), "{error}");
        }
    }

    #[test]
    fn a_login_without_a_server_name_says_so() {
        for host in ["", "  ", "/var/run/postgresql"] {
            let error = target(&kerberos(host)).unwrap_err().to_string();
            assert!(error.contains("server's name"), "{error}");
        }
    }

    #[test]
    fn other_logins_have_no_target() {
        let mut config = kerberos("db");
        config.options.remove(AUTH);
        assert_eq!(target(&config).unwrap(), None);
        config.options.insert(AUTH.to_owned(), "aws_iam".to_owned());
        assert_eq!(target(&config).unwrap(), None);
    }

    fn says(detail: &str) -> String {
        describe(
            &GssError::Failed {
                target: "postgres/db".to_owned(),
                detail: detail.to_owned(),
            },
            false,
        )
    }

    #[test]
    fn a_missing_ticket_says_what_to_run() {
        for raw in [
            "No credentials were supplied, or the credentials were unavailable or inaccessible \
             (No Kerberos credentials available (default cache: FILE:/tmp/krb5cc_1000))",
            "Unspecified GSS failure (No credentials cache found)",
            "Unspecified GSS failure (Credentials cache file '/tmp/none' not found)",
        ] {
            let message = says(raw);
            for what in ["kinit", "KRB5CCNAME", "KRB5_CLIENT_KTNAME", "klist"] {
                assert!(message.contains(what), "{what} in {message}");
            }
            assert!(message.ends_with(raw), "{message}");
        }
    }

    #[test]
    fn the_other_failures_say_what_is_wrong() {
        let message = says("Unspecified (Ticket expired)");
        assert!(
            message.contains("expired") && message.contains("kinit"),
            "{message}"
        );
        let message = says("Unspecified (Server not found in Kerberos database)");
        assert!(message.contains("postgres/db"), "{message}");
        assert!(message.contains("service name"), "{message}");
        let message = says("Unspecified (Cannot contact any KDC for realm 'CORP')");
        assert!(message.contains("krb5.conf"), "{message}");
        let message = says("Unspecified (Configuration file does not specify default realm)");
        assert!(message.contains("default_realm"), "{message}");
    }

    #[test]
    fn windows_says_it_in_any_language() {
        // SSPI's text follows the system's language, so it is not read.
        let message = describe(
            &GssError::Failed {
                target: "postgres/db".to_owned(),
                detail: "Credenziali non disponibili".to_owned(),
            },
            true,
        );
        assert!(message.contains("klist"), "{message}");
        assert!(message.contains("postgres/db"), "{message}");
        assert!(
            message.ends_with("Credenziali non disponibili"),
            "{message}"
        );
    }

    #[test]
    fn a_server_that_does_not_ask_for_it_is_explained() {
        let password = describe(&GssError::PasswordAsked, false);
        assert!(password.contains("none was sent"), "{password}");
        assert!(password.contains("pg_hba.conf"), "{password}");
        let not_asked = describe(&GssError::NotAsked, false);
        assert!(not_asked.contains("without asking"), "{not_asked}");
        let chosen = describe(&GssError::NotChosen, false);
        assert!(chosen.contains("Choose Kerberos"), "{chosen}");
        assert!(describe(&GssError::NotBuilt, false).contains("`kerberos` feature"));
    }
}

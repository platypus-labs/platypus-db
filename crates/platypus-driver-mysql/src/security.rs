//! The accounts and roles of the server, from the `mysql` schema, and the privileges given
//! to one of them, from `information_schema`. MySQL 8 and `MariaDB` keep roles differently:
//! MySQL stores a role as a locked account and its members in `role_edges`, `MariaDB` flags
//! a role in `mysql.user` and keeps its members in `roles_mapping`.

use std::collections::HashMap;

use platypus_core::{
    Connection, GrantKind, Principal, PrincipalFlag, PrincipalKind, PrivilegeGrant, Result,
    TextRowExt, query_text,
};
use platypus_sql::Dialect;

/// MySQL 8's accounts: a role is an account that is locked, expired and has no password.
const MYSQL_ACCOUNTS: &str = "SELECT User, Host, plugin, \
     account_locked = 'Y' AND password_expired = 'Y' AND authentication_string = '', \
     account_locked = 'Y', password_expired = 'Y', Super_priv = 'Y', max_user_connections \
     FROM mysql.user ORDER BY User, Host";

/// Who belongs to which role on MySQL 8: member user, member host, role.
const MYSQL_MEMBERS: &str = "SELECT TO_USER, TO_HOST, FROM_USER FROM mysql.role_edges \
     ORDER BY TO_USER, TO_HOST, FROM_USER";

/// `MariaDB`'s: `is_role`, and the lock only in the JSON of `global_priv`.
const MARIADB_ACCOUNTS: &str = "SELECT u.User, u.Host, u.plugin, u.is_role = 'Y', \
     COALESCE(JSON_VALUE(g.Priv, '$.account_locked'), '0') IN ('1', 'true'), \
     u.password_expired = 'Y', u.Super_priv = 'Y', u.max_user_connections \
     FROM mysql.user u LEFT JOIN mysql.global_priv g ON g.User = u.User AND g.Host = u.Host \
     ORDER BY u.User, u.Host";

const MARIADB_MEMBERS: &str = "SELECT User, Host, Role FROM mysql.roles_mapping \
     WHERE User <> '' ORDER BY User, Host, Role";

pub(crate) async fn principals(conn: &dyn Connection) -> Result<Vec<Principal>> {
    let (accounts, members) = if super::admin::mariadb(conn) {
        (MARIADB_ACCOUNTS, MARIADB_MEMBERS)
    } else {
        (MYSQL_ACCOUNTS, MYSQL_MEMBERS)
    };
    let mut member_of: HashMap<(String, String), Vec<String>> = HashMap::new();
    for r in query_text(conn, members).await? {
        member_of
            .entry((r.text(0), r.text(1)))
            .or_default()
            .push(r.text(2));
    }
    Ok(query_text(conn, accounts)
        .await?
        .into_iter()
        .map(|r| {
            let (name, host) = (r.text(0), r.text(1));
            let role = r.flag(3);
            let mut flags = Vec::new();
            // A role is locked and expired by construction: that says nothing about it.
            for (index, flag) in [
                (4, PrincipalFlag::Locked),
                (5, PrincipalFlag::PasswordExpired),
                (6, PrincipalFlag::Superuser),
            ] {
                if r.flag(index) && (!role || flag == PrincipalFlag::Superuser) {
                    flags.push(flag);
                }
            }
            Principal {
                member_of: member_of
                    .remove(&(name.clone(), host.clone()))
                    .unwrap_or_default(),
                host: Some(host).filter(|h| !h.is_empty()),
                kind: if role {
                    PrincipalKind::Role
                } else {
                    PrincipalKind::User
                },
                flags,
                connection_limit: r
                    .number(7)
                    .filter(|n| *n > 0)
                    .and_then(|n| i64::try_from(n).ok()),
                authentication: r.opt(2).filter(|p| !p.is_empty()),
                name,
                ..Principal::default()
            }
        })
        .collect())
}

/// The global, database, table and column privileges of `'name'@'host'`; `USAGE` (none) left
/// out.
pub(crate) async fn privileges(
    conn: &dyn Connection,
    name: &str,
    host: Option<&str>,
) -> Result<Vec<PrivilegeGrant>> {
    // Roles on `MariaDB` have no host, and show as `'role'@''`.
    let grantee = format!("'{name}'@'{}'", host.unwrap_or(""));
    let grantee = Dialect::MySql.quote_string(&grantee);
    let sql = format!(
        "SELECT 'server', NULL, NULL, NULL, PRIVILEGE_TYPE, IS_GRANTABLE \
         FROM information_schema.USER_PRIVILEGES WHERE GRANTEE = {grantee} \
         UNION ALL SELECT 'database', TABLE_SCHEMA, NULL, NULL, PRIVILEGE_TYPE, IS_GRANTABLE \
         FROM information_schema.SCHEMA_PRIVILEGES WHERE GRANTEE = {grantee} \
         UNION ALL SELECT 'table', TABLE_SCHEMA, TABLE_NAME, NULL, PRIVILEGE_TYPE, IS_GRANTABLE \
         FROM information_schema.TABLE_PRIVILEGES WHERE GRANTEE = {grantee} \
         UNION ALL SELECT 'column', TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, PRIVILEGE_TYPE, \
         IS_GRANTABLE FROM information_schema.COLUMN_PRIVILEGES WHERE GRANTEE = {grantee}"
    );
    let mut grants: Vec<PrivilegeGrant> = query_text(conn, &sql)
        .await?
        .into_iter()
        .filter(|r| r.text(4) != "USAGE")
        .map(|r| PrivilegeGrant {
            kind: match r.text(0).as_str() {
                "server" => GrantKind::Server,
                "database" => GrantKind::Database,
                "table" => GrantKind::Table,
                _ => GrantKind::Column,
            },
            schema: r.opt(1),
            object: r.opt(2),
            column: r.opt(3),
            privilege: r.text(4),
            grantable: r.flag(5),
        })
        .collect();
    sort_grants(&mut grants);
    Ok(grants)
}

/// By object, then privilege: server-wide first.
fn sort_grants(grants: &mut [PrivilegeGrant]) {
    grants.sort_by(|a, b| {
        let key = |g: &PrivilegeGrant| {
            (
                g.kind as u8,
                g.schema.clone(),
                g.object.clone(),
                g.column.clone(),
                g.privilege.clone(),
            )
        };
        key(a).cmp(&key(b))
    });
}

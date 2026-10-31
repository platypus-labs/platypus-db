//! The users and roles of the server, from `system.users` and `system.roles`, and what they
//! have been granted, from `system.grants` and `system.role_grants`. Users defined in the
//! server's configuration files are listed too; the server refuses to change them.

use platypus_core::{
    GrantKind, Principal, PrincipalKind, PrivilegeGrant, Result, TextRowExt, memberships,
    query_text,
};
use platypus_sql::Dialect;

use crate::ClickHouseConnection;

/// Users, then roles: name, how the user authenticates, when its credentials expire.
const ACCOUNTS: &str = "SELECT name, 'user', \
     arrayStringConcat(arrayMap(x -> toString(x), auth_type), ', '), \
     if(empty(valid_until) OR startsWith(toString(valid_until[1]), '1970-01-01'), NULL, \n     toString(valid_until[1])) FROM system.users \
     UNION ALL SELECT name, 'role', NULL, NULL FROM system.roles ORDER BY 2, 1";

/// Member (user or role) and the role it was granted.
const MEMBERS: &str = "SELECT coalesce(user_name, role_name), granted_role_name \
     FROM system.role_grants ORDER BY 1, 2";

pub(crate) async fn principals(conn: &ClickHouseConnection) -> Result<Vec<Principal>> {
    let mut member_of = memberships(conn, MEMBERS).await?;
    let mut list: Vec<Principal> = query_text(conn, ACCOUNTS)
        .await?
        .into_iter()
        .map(|r| {
            let name = r.text(0);
            Principal {
                member_of: member_of.remove(&name).unwrap_or_default(),
                kind: PrincipalKind::of(r.text(1) == "role"),
                authentication: r.opt(2).filter(|a| !a.is_empty()),
                valid_until: r.opt(3),
                name,
                ..Principal::default()
            }
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(list)
}

pub(crate) async fn privileges(
    conn: &ClickHouseConnection,
    name: &str,
) -> Result<Vec<PrivilegeGrant>> {
    let name = Dialect::ClickHouse.quote_literal(name);
    let sql = format!(
        "SELECT database, table, column, access_type, grant_option FROM system.grants \
         WHERE (user_name = {name} OR role_name = {name}) AND NOT is_partial_revoke \
         ORDER BY database, table, column, access_type"
    );
    Ok(query_text(conn, &sql)
        .await?
        .into_iter()
        .map(|r| {
            let (database, table, column) = (r.opt(0), r.opt(1), r.opt(2));
            let kind = match (&database, &table, &column) {
                (None, ..) => GrantKind::Server,
                (_, None, _) => GrantKind::Database,
                (_, _, None) => GrantKind::Table,
                _ => GrantKind::Column,
            };
            PrivilegeGrant {
                kind,
                schema: database,
                object: table,
                column,
                privilege: r.text(3),
                grantable: r.flag(4),
            }
        })
        .collect())
}

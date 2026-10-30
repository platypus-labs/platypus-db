//! The roles of the server, from `pg_roles` and `pg_auth_members`, and the privileges given
//! to one of them, from the access lists (`aclexplode`) of databases, schemas, tables,
//! columns and routines.

use std::collections::HashMap;

use platypus_core::{
    Connection, GrantKind, Principal, PrincipalFlag, PrincipalKind, PrivilegeGrant, Result,
    TextRowExt, query_text,
};
use platypus_sql::Dialect;

/// Every role but the predefined `pg_*` ones: name, can log in, then the other attributes
/// in the order of [`ATTRIBUTES`], connection limit and expiry.
const ROLES: &str = "SELECT rolname, rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, \
     rolinherit, rolreplication, rolbypassrls, NULLIF(rolconnlimit, -1), \
     CASE WHEN rolvaliduntil = 'infinity' THEN NULL \
          ELSE to_char(rolvaliduntil, 'YYYY-MM-DD HH24:MI:SS TZ') END \
     FROM pg_roles WHERE rolname !~ '^pg_' ORDER BY rolname";

/// Columns 2 to 7 of [`ROLES`].
const ATTRIBUTES: [PrincipalFlag; 6] = [
    PrincipalFlag::Superuser,
    PrincipalFlag::CreateDb,
    PrincipalFlag::CreateRole,
    PrincipalFlag::Inherit,
    PrincipalFlag::Replication,
    PrincipalFlag::BypassRls,
];

/// Member and role of every membership; a role granted by several grantors once.
const MEMBERS: &str = "SELECT DISTINCT m.rolname, r.rolname \
     FROM pg_auth_members a \
     JOIN pg_roles r ON r.oid = a.roleid JOIN pg_roles m ON m.oid = a.member \
     ORDER BY 1, 2";

pub(crate) async fn principals(conn: &dyn Connection) -> Result<Vec<Principal>> {
    let mut member_of: HashMap<String, Vec<String>> = HashMap::new();
    for r in query_text(conn, MEMBERS).await? {
        member_of.entry(r.text(0)).or_default().push(r.text(1));
    }
    Ok(query_text(conn, ROLES)
        .await?
        .into_iter()
        .map(|r| {
            let login = r.flag(1);
            let mut flags = Vec::new();
            if login {
                flags.push(PrincipalFlag::Login);
            }
            for (i, flag) in ATTRIBUTES.into_iter().enumerate() {
                if r.flag(i + 2) {
                    flags.push(flag);
                }
            }
            let name = r.text(0);
            Principal {
                member_of: member_of.remove(&name).unwrap_or_default(),
                kind: if login {
                    PrincipalKind::User
                } else {
                    PrincipalKind::Role
                },
                flags,
                connection_limit: r.opt(8).and_then(|n| n.parse().ok()),
                valid_until: r.opt(9),
                name,
                ..Principal::default()
            }
        })
        .collect())
}

/// The privileges of role `name` that its own access lists name. An object nobody granted
/// anything on has no list, and what the default gives (`PUBLIC`) is not the role's.
const PRIVILEGES: &str = "WITH r AS (SELECT oid FROM pg_roles WHERE rolname = {name}) \
     SELECT 'database', NULL, d.datname, NULL, a.privilege_type, a.is_grantable \
       FROM pg_database d, aclexplode(d.datacl) a, r WHERE a.grantee = r.oid \
     UNION ALL \
     SELECT 'schema', NULL, n.nspname, NULL, a.privilege_type, a.is_grantable \
       FROM pg_namespace n, aclexplode(n.nspacl) a, r WHERE a.grantee = r.oid \
     UNION ALL \
     SELECT 'table', n.nspname, c.relname, NULL, a.privilege_type, a.is_grantable \
       FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace, \
            aclexplode(c.relacl) a, r \
       WHERE a.grantee = r.oid AND c.relkind IN ('r', 'v', 'm', 'f', 'p') \
         AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
     UNION ALL \
     SELECT 'column', n.nspname, c.relname, t.attname, a.privilege_type, a.is_grantable \
       FROM pg_attribute t JOIN pg_class c ON c.oid = t.attrelid \
            JOIN pg_namespace n ON n.oid = c.relnamespace, \
            aclexplode(t.attacl) a, r \
       WHERE a.grantee = r.oid AND NOT t.attisdropped \
         AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
     UNION ALL \
     SELECT 'routine', n.nspname, \
            p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')', \
            NULL, a.privilege_type, a.is_grantable \
       FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace, \
            aclexplode(p.proacl) a, r \
       WHERE a.grantee = r.oid AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
     ORDER BY 1, 2, 3, 4, 5";

pub(crate) async fn privileges(conn: &dyn Connection, name: &str) -> Result<Vec<PrivilegeGrant>> {
    let sql = PRIVILEGES.replace("{name}", &Dialect::Postgres.quote_literal(name));
    Ok(query_text(conn, &sql)
        .await?
        .into_iter()
        .map(|r| {
            let kind = match r.text(0).as_str() {
                "database" => GrantKind::Database,
                "schema" => GrantKind::Schema,
                "table" => GrantKind::Table,
                "column" => GrantKind::Column,
                _ => GrantKind::Routine,
            };
            // A database or a schema is named in its own column: the object is its name.
            let (schema, object) = if matches!(kind, GrantKind::Database | GrantKind::Schema) {
                (Some(r.text(2)), None)
            } else {
                (r.opt(1), r.opt(2))
            };
            PrivilegeGrant {
                kind,
                schema,
                object,
                column: r.opt(3),
                privilege: r.text(4),
                grantable: r.flag(5),
            }
        })
        .collect())
}

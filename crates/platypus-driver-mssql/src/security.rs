//! The users and roles of the database the session is in, from `sys.database_principals`
//! and `sys.database_role_members`, and the permissions given to one of them, from
//! `sys.database_permissions`. A user that logs in is joined to its server login, whose
//! state (disabled) it shows. Server-level logins and permissions are not read.

use platypus_core::{
    Connection, GrantKind, Principal, PrincipalFlag, PrincipalKind, PrivilegeGrant, Result,
    TextRowExt, memberships, query_text,
};
use platypus_sql::Dialect;

/// The users and the roles made in the database: the fixed roles, `dbo`, `guest` and the
/// system schemas are left out. Name, type (`R` is a role), how it authenticates, whether
/// its login is disabled, whether it has a login and the login's name.
const PRINCIPALS: &str = "SELECT p.name, p.type, p.authentication_type_desc, \
     CAST(ISNULL(l.is_disabled, 0) AS int), CAST(CASE WHEN l.sid IS NULL THEN 0 ELSE 1 END AS int), l.name \
     FROM sys.database_principals p LEFT JOIN sys.server_principals l ON l.sid = p.sid \
     WHERE p.type IN ('S', 'U', 'G', 'E', 'X', 'R') AND p.is_fixed_role = 0 \
       AND p.name NOT IN ('dbo', 'guest', 'INFORMATION_SCHEMA', 'sys', 'public') \
     ORDER BY p.name";

/// Member and role of every membership, the fixed roles' too.
const MEMBERS: &str = "SELECT m.name, r.name FROM sys.database_role_members rm \
     JOIN sys.database_principals r ON r.principal_id = rm.role_principal_id \
     JOIN sys.database_principals m ON m.principal_id = rm.member_principal_id \
     ORDER BY 1, 2";

/// The grants (denials are left out) to the principal named `{name}`: class, schema, object,
/// column, permission and whether it can be passed on.
const PERMISSIONS: &str = "SELECT pr.class_desc, s.name, o.name, c.name, pr.permission_name, \
     CAST(CASE WHEN pr.state = 'W' THEN 1 ELSE 0 END AS int) \
     FROM sys.database_permissions pr \
     LEFT JOIN sys.objects o ON pr.class = 1 AND o.object_id = pr.major_id \
     LEFT JOIN sys.schemas s ON (pr.class = 1 AND s.schema_id = o.schema_id) \
                             OR (pr.class = 3 AND s.schema_id = pr.major_id) \
     LEFT JOIN sys.columns c ON pr.class = 1 AND c.object_id = pr.major_id \
                             AND c.column_id = pr.minor_id AND pr.minor_id > 0 \
     WHERE pr.grantee_principal_id = DATABASE_PRINCIPAL_ID({name}) AND pr.state IN ('G', 'W') \
       AND pr.class IN (0, 1, 3) \
     ORDER BY pr.class, s.name, o.name, c.name, pr.permission_name";

pub(crate) async fn principals(conn: &dyn Connection) -> Result<Vec<Principal>> {
    let mut member_of = memberships(conn, MEMBERS).await?;
    Ok(query_text(conn, PRINCIPALS)
        .await?
        .into_iter()
        .map(|r| {
            let role = r.text(1) == "R";
            let mut flags = Vec::new();
            if !role && r.flag(4) {
                flags.push(PrincipalFlag::Login);
            }
            if r.flag(3) {
                flags.push(PrincipalFlag::Locked);
            }
            let name = r.text(0);
            Principal {
                member_of: member_of.remove(&name).unwrap_or_default(),
                kind: PrincipalKind::of(role),
                flags,
                authentication: Some(r.text(2)).filter(|a| !a.is_empty() && !role),
                login: r.opt(5).filter(|_| !role),
                name,
                ..Principal::default()
            }
        })
        .collect())
}

pub(crate) async fn privileges(conn: &dyn Connection, name: &str) -> Result<Vec<PrivilegeGrant>> {
    let sql = PERMISSIONS.replace("{name}", &Dialect::SqlServer.quote_literal(name));
    Ok(query_text(conn, &sql)
        .await?
        .into_iter()
        .map(|r| {
            let kind = match (r.text(0).as_str(), r.opt(3).is_some()) {
                ("DATABASE", _) => GrantKind::Database,
                ("SCHEMA", _) => GrantKind::Schema,
                (_, true) => GrantKind::Column,
                _ => GrantKind::Table,
            };
            PrivilegeGrant {
                kind,
                schema: r.opt(1),
                object: r.opt(2),
                column: r.opt(3),
                privilege: r.text(4),
                grantable: r.flag(5),
            }
        })
        .collect())
}

//! The statements that create, change and drop users and roles and that grant and revoke
//! privileges, for PostgreSQL, MySQL and `MariaDB`, SQL Server and `ClickHouse`
//! ([`statements`]); and [`mask_secrets`], which hides passwords from the query log.
//!
//! The app shows the statements before they run and writes them to its query log; a
//! password is in the statement that runs, and nowhere else.

use std::borrow::Cow;
use std::fmt::Write as _;

use platypus_core::{
    DropOptions, GrantAction, GrantTarget, Grantee, PrincipalFlag, PrincipalKind, PrincipalSpec,
    SecurityChange, ServerInfo, ServerVersion, TargetClass,
};

use crate::Dialect;

/// What a password is shown as in a preview.
const MASK: &str = "'********'";

/// Why a change cannot be written for this database: shown to the user as it is.
pub type Unsupported = String;

/// The statements that make `change`, in the order they run. With `mask`, a password is
/// written as `'********'`: that is the form to show or log, not to run.
///
/// # Errors
/// When the name is empty, or the database has no such attribute, privilege or object
/// kind.
pub fn statements(
    dialect: Dialect,
    change: &SecurityChange,
    mask: bool,
) -> Result<Vec<String>, Unsupported> {
    if !matches!(
        dialect,
        Dialect::Postgres | Dialect::MySql | Dialect::SqlServer | Dialect::ClickHouse
    ) {
        return Err("users and roles are not supported for this database".to_owned());
    }
    let w = Writer { dialect, mask };
    match change {
        SecurityChange::Create { spec } => w.create(spec),
        SecurityChange::Alter { spec } => w.alter(spec),
        SecurityChange::Drop {
            name,
            host,
            kind,
            options,
        } => w.drop(name, host.as_deref(), *kind, options),
        SecurityChange::Privileges {
            action,
            grantees,
            target,
            privileges,
            grant_option,
            cascade,
        } => w.privileges(
            *action,
            grantees,
            target,
            privileges,
            Options {
                grant_option: *grant_option,
                cascade: *cascade,
            },
        ),
        SecurityChange::Membership {
            action,
            role,
            members,
            admin_option,
        } => w.membership(*action, role, members, *admin_option),
    }
}

/// The privileges a grant on `class` can name in `dialect`, as the database writes them;
/// empty when the database has no such grants. `ALL` is among them where it exists.
#[must_use]
pub fn privileges(dialect: Dialect, class: TargetClass) -> &'static [&'static str] {
    match dialect {
        Dialect::Postgres => postgres_privileges(class),
        Dialect::MySql => mysql_privileges(class),
        Dialect::SqlServer => sql_server_privileges(class),
        Dialect::ClickHouse => clickhouse_privileges(class),
        _ => &[],
    }
}

fn postgres_privileges(class: TargetClass) -> &'static [&'static str] {
    match class {
        TargetClass::Database => &["CREATE", "CONNECT", "TEMPORARY", "ALL"],
        TargetClass::Schema => &["USAGE", "CREATE", "ALL"],
        TargetClass::Table | TargetClass::AllTables => &[
            "SELECT",
            "INSERT",
            "UPDATE",
            "DELETE",
            "TRUNCATE",
            "REFERENCES",
            "TRIGGER",
            "ALL",
        ],
        TargetClass::AllRoutines => &["EXECUTE", "ALL"],
        TargetClass::Server => &[],
    }
}

/// What a MySQL grant on a table takes.
const MYSQL_TABLE: &[&str] = &[
    "ALL",
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "INDEX",
    "REFERENCES",
    "TRIGGER",
    "CREATE VIEW",
    "SHOW VIEW",
];

/// On a database: the table ones and those for routines and events.
const MYSQL_DATABASE: &[&str] = &[
    "ALL",
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "INDEX",
    "REFERENCES",
    "TRIGGER",
    "EXECUTE",
    "CREATE VIEW",
    "SHOW VIEW",
    "CREATE ROUTINE",
    "ALTER ROUTINE",
    "EVENT",
    "LOCK TABLES",
    "CREATE TEMPORARY TABLES",
];

/// On the server: the database ones and those that administer it.
const MYSQL_SERVER: &[&str] = &[
    "ALL",
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "INDEX",
    "REFERENCES",
    "TRIGGER",
    "EXECUTE",
    "CREATE VIEW",
    "SHOW VIEW",
    "CREATE ROUTINE",
    "ALTER ROUTINE",
    "EVENT",
    "LOCK TABLES",
    "CREATE TEMPORARY TABLES",
    "PROCESS",
    "RELOAD",
    "SHUTDOWN",
    "SUPER",
    "FILE",
    "REPLICATION SLAVE",
    "REPLICATION CLIENT",
    "CREATE USER",
    "SHOW DATABASES",
];

fn mysql_privileges(class: TargetClass) -> &'static [&'static str] {
    match class {
        TargetClass::Server => MYSQL_SERVER,
        TargetClass::Table => MYSQL_TABLE,
        _ => MYSQL_DATABASE,
    }
}

fn sql_server_privileges(class: TargetClass) -> &'static [&'static str] {
    match class {
        TargetClass::Database => &[
            "CONNECT",
            "CREATE TABLE",
            "CREATE VIEW",
            "CREATE PROCEDURE",
            "CREATE SCHEMA",
            "SELECT",
            "INSERT",
            "UPDATE",
            "DELETE",
            "EXECUTE",
            "ALTER",
            "CONTROL",
            "VIEW DEFINITION",
        ],
        TargetClass::Schema => &[
            "SELECT",
            "INSERT",
            "UPDATE",
            "DELETE",
            "EXECUTE",
            "REFERENCES",
            "ALTER",
            "CONTROL",
            "VIEW DEFINITION",
        ],
        TargetClass::AllTables => &[
            "SELECT",
            "INSERT",
            "UPDATE",
            "DELETE",
            "REFERENCES",
            "ALTER",
            "VIEW DEFINITION",
        ],
        TargetClass::AllRoutines => &["EXECUTE", "ALTER", "VIEW DEFINITION"],
        TargetClass::Table => &[
            "SELECT",
            "INSERT",
            "UPDATE",
            "DELETE",
            "REFERENCES",
            "ALTER",
            "CONTROL",
            "TAKE OWNERSHIP",
            "VIEW DEFINITION",
            "ALL",
        ],
        TargetClass::Server => &[],
    }
}

fn clickhouse_privileges(class: TargetClass) -> &'static [&'static str] {
    match class {
        TargetClass::Server
        | TargetClass::Database
        | TargetClass::Schema
        | TargetClass::AllTables => &[
            "ALL", "SELECT", "INSERT", "ALTER", "CREATE", "DROP", "TRUNCATE", "SHOW", "OPTIMIZE",
        ],
        TargetClass::Table => &[
            "ALL", "SELECT", "INSERT", "ALTER", "DROP", "TRUNCATE", "SHOW", "OPTIMIZE",
        ],
        TargetClass::AllRoutines => &[],
    }
}

/// The attributes of a user or role that `dialect` has, in the order a form shows them.
#[must_use]
pub fn flags(dialect: Dialect) -> &'static [PrincipalFlag] {
    match dialect {
        Dialect::Postgres => &[
            PrincipalFlag::Login,
            PrincipalFlag::Superuser,
            PrincipalFlag::CreateDb,
            PrincipalFlag::CreateRole,
            PrincipalFlag::Inherit,
            PrincipalFlag::Replication,
            PrincipalFlag::BypassRls,
        ],
        Dialect::MySql => &[PrincipalFlag::Locked, PrincipalFlag::PasswordExpired],
        Dialect::SqlServer => &[PrincipalFlag::Locked],
        _ => &[],
    }
}

/// The two ways a revoke differs from a grant.
#[derive(Clone, Copy)]
struct Options {
    grant_option: bool,
    cascade: bool,
}

/// A dialect, and whether passwords are masked.
struct Writer {
    dialect: Dialect,
    mask: bool,
}

impl Writer {
    fn ident(&self, name: &str) -> String {
        self.dialect.quote_ident(name).into_owned()
    }

    /// A string in a place the grammar takes no expression.
    fn text(&self, value: &str) -> String {
        self.dialect.quote_string(value)
    }

    fn password(&self, password: &str) -> String {
        if self.mask {
            MASK.to_owned()
        } else {
            self.text(password)
        }
    }

    /// `'name'@'host'` for a MySQL account, the quoted name elsewhere.
    fn principal(&self, name: &str, host: Option<&str>) -> String {
        if self.dialect == Dialect::MySql {
            match host.filter(|h| !h.is_empty()) {
                Some(host) => format!("{}@{}", self.text(name), self.text(host)),
                None => self.text(name),
            }
        } else {
            self.ident(name)
        }
    }

    fn grantee(&self, g: &Grantee) -> Result<String, Unsupported> {
        check_name(&g.name)?;
        Ok(self.principal(&g.name, g.host.as_deref()))
    }

    fn grantees(&self, list: &[Grantee]) -> Result<String, Unsupported> {
        if list.is_empty() {
            return Err("name at least one user or role".to_owned());
        }
        let names: Result<Vec<String>, _> = list.iter().map(|g| self.grantee(g)).collect();
        Ok(names?.join(", "))
    }

    /// Refuses attributes the database does not have, rather than dropping them silently.
    fn check_flags(&self, spec: &PrincipalSpec) -> Result<(), Unsupported> {
        let known = flags(self.dialect);
        for flag in spec.flags_on.iter().chain(&spec.flags_off) {
            if !known.contains(flag) {
                return Err(format!("{flag:?} is not an attribute of this database"));
            }
        }
        Ok(())
    }

    fn create(&self, spec: &PrincipalSpec) -> Result<Vec<String>, Unsupported> {
        check_name(&spec.name)?;
        self.check_flags(spec)?;
        match self.dialect {
            Dialect::Postgres => Ok(vec![self.pg_create(spec)]),
            Dialect::MySql => Ok(vec![self.mysql_create(spec)?]),
            Dialect::SqlServer => self.mssql_create(spec),
            _ => Ok(vec![self.clickhouse_create(spec)?]),
        }
    }

    fn alter(&self, spec: &PrincipalSpec) -> Result<Vec<String>, Unsupported> {
        check_name(&spec.name)?;
        self.check_flags(spec)?;
        let out = match self.dialect {
            Dialect::Postgres => self.pg_alter(spec),
            Dialect::MySql => self.mysql_alter(spec)?,
            Dialect::SqlServer => self.mssql_alter(spec)?,
            _ => self.clickhouse_alter(spec)?,
        };
        if out.is_empty() {
            Err("nothing to change".to_owned())
        } else {
            Ok(out)
        }
    }

    fn drop(
        &self,
        name: &str,
        host: Option<&str>,
        kind: PrincipalKind,
        options: &DropOptions,
    ) -> Result<Vec<String>, Unsupported> {
        check_name(name)?;
        // A role has no host: `MariaDB` rejects one, and MySQL means `%`.
        let who = self.principal(name, host.filter(|_| kind == PrincipalKind::User));
        let noun = match kind {
            PrincipalKind::User if self.dialect != Dialect::Postgres => "USER",
            _ => "ROLE",
        };
        let mut out = Vec::new();
        match self.dialect {
            Dialect::Postgres => {
                if let Some(to) = options.reassign_to.as_deref().filter(|t| !t.is_empty()) {
                    out.push(format!("REASSIGN OWNED BY {who} TO {}", self.ident(to)));
                }
                if options.drop_owned {
                    out.push(format!("DROP OWNED BY {who}"));
                }
                out.push(format!("DROP ROLE {who}"));
            }
            Dialect::SqlServer => {
                out.push(format!("DROP {noun} {who}"));
                // The login can have another name than the user's; without one there is
                // nothing to drop.
                if let Some(login) = options
                    .login
                    .as_deref()
                    .filter(|l| options.drop_login && kind == PrincipalKind::User && !l.is_empty())
                {
                    out.push(format!("DROP LOGIN {}", self.ident(login)));
                }
            }
            _ => out.push(format!("DROP {noun} {who}")),
        }
        Ok(out)
    }

    fn privileges(
        &self,
        action: GrantAction,
        grantees: &[Grantee],
        target: &GrantTarget,
        names: &[String],
        options: Options,
    ) -> Result<Vec<String>, Unsupported> {
        let to = self.grantees(grantees)?;
        let list = self.privilege_list(target.class(), names)?;
        let on = self.target(target)?;
        let on = if on.is_empty() {
            String::new()
        } else {
            format!(" ON {on}")
        };
        Ok(vec![match action {
            GrantAction::Grant => {
                let option = if options.grant_option {
                    " WITH GRANT OPTION"
                } else {
                    ""
                };
                format!("GRANT {list}{on} TO {to}{option}")
            }
            GrantAction::Revoke => self.revoke(&list, &on, &to, options)?,
        }])
    }

    fn revoke(
        &self,
        list: &str,
        on: &str,
        from: &str,
        options: Options,
    ) -> Result<String, Unsupported> {
        let cascade =
            if options.cascade && matches!(self.dialect, Dialect::Postgres | Dialect::SqlServer) {
                " CASCADE"
            } else {
                ""
            };
        match (self.dialect, options.grant_option) {
            // MySQL can only take the grant option away from everything on the object.
            (Dialect::MySql, true) => Ok(format!("REVOKE GRANT OPTION{on} FROM {from}")),
            (Dialect::ClickHouse, true) => {
                Err("ClickHouse cannot revoke only the grant option".to_owned())
            }
            (_, true) => Ok(format!(
                "REVOKE GRANT OPTION FOR {list}{on} FROM {from}{cascade}"
            )),
            (_, false) => Ok(format!("REVOKE {list}{on} FROM {from}{cascade}")),
        }
    }

    /// The privileges as written, each checked against what the database has for the
    /// kind of object.
    fn privilege_list(&self, class: TargetClass, names: &[String]) -> Result<String, Unsupported> {
        let allowed = privileges(self.dialect, class);
        if allowed.is_empty() {
            return Err("this database has no such privileges on that kind of object".to_owned());
        }
        if names.is_empty() {
            return Err("choose at least one privilege".to_owned());
        }
        let mut out: Vec<String> = Vec::new();
        for name in names {
            let wanted = name.trim().to_uppercase();
            let Some(found) = allowed.iter().find(|a| **a == wanted) else {
                return Err(format!("{wanted} cannot be granted on that object"));
            };
            let found = (*found).to_owned();
            // SQL Server's `ALL` is deprecated: write what it stands for on a table.
            let expanded = if self.dialect == Dialect::SqlServer && found == "ALL" {
                vec!["SELECT", "INSERT", "UPDATE", "DELETE", "REFERENCES"]
            } else if found == "ALL" {
                vec!["ALL"]
            } else {
                vec![found.as_str()]
            };
            for p in expanded {
                if !out.iter().any(|o| o == p) {
                    out.push(p.to_owned());
                }
            }
        }
        // `ALL` already says it, and `ALL, SELECT` is a syntax error in MySQL.
        if out.iter().any(|o| o == "ALL") {
            out = vec!["ALL".to_owned()];
        }
        Ok(out.join(", "))
    }

    /// The object a privilege is on, as `ON` takes it; empty where there is none (the
    /// database of a SQL Server session).
    fn target(&self, target: &GrantTarget) -> Result<String, Unsupported> {
        let d = self.dialect;
        let q = |n: &str| self.ident(n);
        let schema_scope = |schema: &str| match d {
            Dialect::Postgres => format!("SCHEMA {}", q(schema)),
            Dialect::SqlServer => format!("SCHEMA::{}", q(schema)),
            _ => format!("{}.*", q(schema)),
        };
        Ok(match target {
            GrantTarget::Server => match d {
                Dialect::MySql | Dialect::ClickHouse => "*.*".to_owned(),
                _ => return Err("this database has no server-wide grants here".to_owned()),
            },
            GrantTarget::Database { name } => match d {
                Dialect::Postgres => format!("DATABASE {}", q(name)),
                Dialect::SqlServer => String::new(),
                _ => format!("{}.*", q(name)),
            },
            GrantTarget::Schema { name } => schema_scope(name),
            GrantTarget::Table { schema, name } => match d {
                Dialect::Postgres => format!("TABLE {}.{}", q(schema), q(name)),
                Dialect::SqlServer => format!("OBJECT::{}.{}", q(schema), q(name)),
                _ => format!("{}.{}", q(schema), q(name)),
            },
            GrantTarget::AllTables { schema } => match d {
                Dialect::Postgres => format!("ALL TABLES IN SCHEMA {}", q(schema)),
                _ => schema_scope(schema),
            },
            GrantTarget::AllRoutines { schema } => match d {
                Dialect::Postgres => format!("ALL ROUTINES IN SCHEMA {}", q(schema)),
                Dialect::ClickHouse => return Err("ClickHouse has no routine grants".to_owned()),
                _ => schema_scope(schema),
            },
        })
    }

    fn membership(
        &self,
        action: GrantAction,
        role: &str,
        members: &[Grantee],
        admin_option: bool,
    ) -> Result<Vec<String>, Unsupported> {
        check_name(role)?;
        if members.is_empty() {
            return Err("name at least one user or role".to_owned());
        }
        if self.dialect == Dialect::SqlServer {
            if admin_option {
                return Err("SQL Server has no admin option on roles".to_owned());
            }
            let verb = match action {
                GrantAction::Grant => "ADD",
                GrantAction::Revoke => "DROP",
            };
            let role = self.ident(role);
            return members
                .iter()
                .map(|m| {
                    Ok(format!(
                        "ALTER ROLE {role} {verb} MEMBER {}",
                        self.grantee(m)?
                    ))
                })
                .collect();
        }
        let role = self.principal(role, None);
        let to = self.grantees(members)?;
        Ok(vec![match (action, admin_option) {
            (GrantAction::Grant, false) => format!("GRANT {role} TO {to}"),
            (GrantAction::Grant, true) => format!("GRANT {role} TO {to} WITH ADMIN OPTION"),
            (GrantAction::Revoke, false) => format!("REVOKE {role} FROM {to}"),
            (GrantAction::Revoke, true) if self.dialect == Dialect::MySql => {
                return Err("MySQL cannot revoke only the admin option".to_owned());
            }
            (GrantAction::Revoke, true) => format!("REVOKE ADMIN OPTION FOR {role} FROM {to}"),
        }])
    }
}

/// PostgreSQL.
impl Writer {
    /// The `LOGIN`, `SUPERUSER`, … words for what `spec` turns on, and with `both` what it
    /// turns off.
    fn pg_attributes(spec: &PrincipalSpec, both: bool) -> Vec<&'static str> {
        const WORDS: [(PrincipalFlag, &str, &str); 7] = [
            (PrincipalFlag::Login, "LOGIN", "NOLOGIN"),
            (PrincipalFlag::Superuser, "SUPERUSER", "NOSUPERUSER"),
            (PrincipalFlag::CreateDb, "CREATEDB", "NOCREATEDB"),
            (PrincipalFlag::CreateRole, "CREATEROLE", "NOCREATEROLE"),
            (PrincipalFlag::Inherit, "INHERIT", "NOINHERIT"),
            (PrincipalFlag::Replication, "REPLICATION", "NOREPLICATION"),
            (PrincipalFlag::BypassRls, "BYPASSRLS", "NOBYPASSRLS"),
        ];
        let mut out = Vec::new();
        for (flag, on, off) in WORDS {
            if spec.flags_on.contains(&flag) {
                out.push(on);
            } else if (both || flag == PrincipalFlag::Inherit) && spec.flags_off.contains(&flag) {
                out.push(off);
            }
        }
        out
    }

    /// `CONNECTION LIMIT`, `PASSWORD` and `VALID UNTIL`, for what `spec` sets.
    fn pg_settings(&self, spec: &PrincipalSpec, creating: bool) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(n) = spec.connection_limit.filter(|n| !creating || *n >= 0) {
            out.push(format!("CONNECTION LIMIT {}", n.max(-1)));
        }
        if let Some(p) = non_empty(spec.password.as_deref()) {
            out.push(format!("PASSWORD {}", self.password_literal(p)));
        }
        match spec.valid_until.as_deref().map(str::trim) {
            Some("") if !creating => out.push("VALID UNTIL 'infinity'".to_owned()),
            Some(ts) if !ts.is_empty() => {
                out.push(format!("VALID UNTIL {}", self.dialect.quote_literal(ts)));
            }
            _ => {}
        }
        out
    }

    /// A password the way PostgreSQL reads it whatever `standard_conforming_strings` is.
    fn password_literal(&self, password: &str) -> String {
        if self.mask {
            MASK.to_owned()
        } else {
            self.dialect.quote_literal(password)
        }
    }

    fn pg_create(&self, spec: &PrincipalSpec) -> String {
        let user = spec.kind == PrincipalKind::User;
        let mut words = Self::pg_attributes(spec, false)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        // `CREATE USER` logs in unless told otherwise.
        if user && spec.flags_off.contains(&PrincipalFlag::Login) {
            words.insert(0, "NOLOGIN".to_owned());
        }
        words.extend(self.pg_settings(spec, true));
        let noun = if user { "USER" } else { "ROLE" };
        let head = format!("CREATE {noun} {}", self.ident(&spec.name));
        if words.is_empty() {
            head
        } else {
            format!("{head} WITH {}", words.join(" "))
        }
    }

    fn pg_alter(&self, spec: &PrincipalSpec) -> Vec<String> {
        let name = self.ident(&spec.name);
        let mut words = Self::pg_attributes(spec, true)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        words.extend(self.pg_settings(spec, false));
        let mut out = Vec::new();
        if !words.is_empty() {
            out.push(format!("ALTER ROLE {name} WITH {}", words.join(" ")));
        }
        if let Some(new) = non_empty(spec.rename_to.as_deref()) {
            out.push(format!("ALTER ROLE {name} RENAME TO {}", self.ident(new)));
        }
        out
    }
}

/// MySQL and `MariaDB`.
impl Writer {
    /// The words after the account that set a password, a limit and the lock.
    fn mysql_options(&self, spec: &PrincipalSpec) -> Result<Vec<String>, Unsupported> {
        if non_empty(spec.valid_until.as_deref()).is_some() {
            return Err("MySQL has no expiry date for an account's password here".to_owned());
        }
        let mut out = Vec::new();
        if let Some(p) = non_empty(spec.password.as_deref()) {
            out.push(format!("IDENTIFIED BY {}", self.password(p)));
        }
        if let Some(n) = spec.connection_limit {
            // 0 is "no limit" for MAX_USER_CONNECTIONS.
            out.push(format!("WITH MAX_USER_CONNECTIONS {}", n.max(0)));
        }
        if spec.flags_on.contains(&PrincipalFlag::Locked) {
            out.push("ACCOUNT LOCK".to_owned());
        } else if spec.flags_off.contains(&PrincipalFlag::Locked) {
            out.push("ACCOUNT UNLOCK".to_owned());
        }
        if spec.flags_on.contains(&PrincipalFlag::PasswordExpired) {
            out.push("PASSWORD EXPIRE".to_owned());
        }
        Ok(out)
    }

    fn mysql_create(&self, spec: &PrincipalSpec) -> Result<String, Unsupported> {
        if spec.kind == PrincipalKind::Role {
            return Ok(format!("CREATE ROLE {}", self.principal(&spec.name, None)));
        }
        let mut words = vec![format!(
            "CREATE USER {}",
            self.principal(&spec.name, spec.host.as_deref())
        )];
        words.extend(self.mysql_options(spec)?);
        Ok(words.join(" "))
    }

    fn mysql_alter(&self, spec: &PrincipalSpec) -> Result<Vec<String>, Unsupported> {
        let host = spec
            .host
            .as_deref()
            .filter(|_| spec.kind == PrincipalKind::User);
        let who = self.principal(&spec.name, host);
        let options = self.mysql_options(spec)?;
        let mut out = Vec::new();
        if !options.is_empty() {
            out.push(format!("ALTER USER {who} {}", options.join(" ")));
        }
        if let Some(new) = non_empty(spec.rename_to.as_deref()) {
            let to = self.principal(new, host);
            out.push(format!("RENAME USER {who} TO {to}"));
        }
        Ok(out)
    }
}

/// SQL Server.
impl Writer {
    fn mssql_create(&self, spec: &PrincipalSpec) -> Result<Vec<String>, Unsupported> {
        if spec.connection_limit.is_some() || non_empty(spec.valid_until.as_deref()).is_some() {
            return Err("SQL Server has no connection limit or expiry date for a user".to_owned());
        }
        let name = self.ident(&spec.name);
        if spec.kind == PrincipalKind::Role {
            return Ok(vec![format!("CREATE ROLE {name}")]);
        }
        let Some(password) = non_empty(spec.password.as_deref()) else {
            return Ok(vec![format!("CREATE USER {name} WITHOUT LOGIN")]);
        };
        let mut out = vec![
            format!(
                "CREATE LOGIN {name} WITH PASSWORD = {}",
                self.password_literal(password)
            ),
            format!("CREATE USER {name} FOR LOGIN {name}"),
        ];
        if spec.flags_on.contains(&PrincipalFlag::Locked) {
            out.push(format!("ALTER LOGIN {name} DISABLE"));
        }
        Ok(out)
    }

    fn mssql_alter(&self, spec: &PrincipalSpec) -> Result<Vec<String>, Unsupported> {
        if spec.connection_limit.is_some() || non_empty(spec.valid_until.as_deref()).is_some() {
            return Err("SQL Server has no connection limit or expiry date for a user".to_owned());
        }
        let name = self.ident(&spec.name);
        let mut out = Vec::new();
        let wants_login_change = non_empty(spec.password.as_deref()).is_some()
            || spec.flags_on.contains(&PrincipalFlag::Locked)
            || spec.flags_off.contains(&PrincipalFlag::Locked);
        if wants_login_change && spec.kind == PrincipalKind::User {
            // The login is not necessarily named like the user.
            let Some(login) = non_empty(spec.login.as_deref()) else {
                return Err("the user has no server login to change".to_owned());
            };
            let login = self.ident(login);
            if let Some(p) = non_empty(spec.password.as_deref()) {
                out.push(format!(
                    "ALTER LOGIN {login} WITH PASSWORD = {}",
                    self.password_literal(p)
                ));
            }
            if spec.flags_on.contains(&PrincipalFlag::Locked) {
                out.push(format!("ALTER LOGIN {login} DISABLE"));
            } else if spec.flags_off.contains(&PrincipalFlag::Locked) {
                out.push(format!("ALTER LOGIN {login} ENABLE"));
            }
        }
        if let Some(new) = non_empty(spec.rename_to.as_deref()) {
            let noun = if spec.kind == PrincipalKind::Role {
                "ROLE"
            } else {
                "USER"
            };
            out.push(format!(
                "ALTER {noun} {name} WITH NAME = {}",
                self.ident(new)
            ));
        }
        Ok(out)
    }
}

/// `ClickHouse`.
impl Writer {
    /// The `VALID UNTIL` clause, and the authentication clause where one is asked for.
    fn clickhouse_options(&self, spec: &PrincipalSpec) -> Result<Vec<String>, Unsupported> {
        if spec.connection_limit.is_some() {
            return Err("ClickHouse has no connection limit for a user".to_owned());
        }
        let mut out = Vec::new();
        if let Some(p) = non_empty(spec.password.as_deref()) {
            out.push(format!(
                "IDENTIFIED WITH sha256_password BY {}",
                self.password(p)
            ));
        }
        match spec.valid_until.as_deref().map(str::trim) {
            Some("") => out.push("VALID UNTIL 'infinity'".to_owned()),
            Some(ts) => out.push(format!("VALID UNTIL {}", self.dialect.quote_literal(ts))),
            None => {}
        }
        Ok(out)
    }

    fn clickhouse_create(&self, spec: &PrincipalSpec) -> Result<String, Unsupported> {
        let name = self.ident(&spec.name);
        if spec.kind == PrincipalKind::Role {
            return Ok(format!("CREATE ROLE {name}"));
        }
        let mut options = self.clickhouse_options(spec)?;
        if non_empty(spec.password.as_deref()).is_none() {
            options.insert(0, "NOT IDENTIFIED".to_owned());
        }
        Ok(format!("CREATE USER {name} {}", options.join(" ")))
    }

    fn clickhouse_alter(&self, spec: &PrincipalSpec) -> Result<Vec<String>, Unsupported> {
        let noun = if spec.kind == PrincipalKind::Role {
            "ROLE"
        } else {
            "USER"
        };
        let name = self.ident(&spec.name);
        let options = self.clickhouse_options(spec)?;
        let mut out = Vec::new();
        if !options.is_empty() {
            out.push(format!("ALTER {noun} {name} {}", options.join(" ")));
        }
        if let Some(new) = non_empty(spec.rename_to.as_deref()) {
            out.push(format!("ALTER {noun} {name} RENAME TO {}", self.ident(new)));
        }
        Ok(out)
    }
}

fn non_empty(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

fn check_name(name: &str) -> Result<(), Unsupported> {
    if name.trim().is_empty() {
        Err("the name is empty".to_owned())
    } else if name.chars().any(char::is_control) {
        Err("the name has a control character".to_owned())
    } else {
        Ok(())
    }
}

/// Whether the server has roles: every database here does, except MySQL before 8.0.
/// `MariaDB` and `TiDB` number their versions apart and have them.
#[must_use]
pub fn roles_supported(dialect: Dialect, info: &ServerInfo) -> bool {
    dialect != Dialect::MySql
        || info.product == "MariaDB"
        || info.version.contains("TiDB")
        || info.at_least(ServerVersion::new(8, 0, 0))
}

/// Whether `change` creates, alters, drops or grants a role, or changes who belongs to one.
#[must_use]
pub fn involves_roles(change: &SecurityChange) -> bool {
    match change {
        SecurityChange::Create { spec } | SecurityChange::Alter { spec } => {
            spec.kind == PrincipalKind::Role
        }
        SecurityChange::Drop { kind, .. } => *kind == PrincipalKind::Role,
        SecurityChange::Membership { .. } => true,
        SecurityChange::Privileges { .. } => false,
    }
}

/// `sql` with its credentials replaced by `'********'`, so that it can go to a log: the
/// passwords of `CREATE`/`ALTER` `USER`/`ROLE`/`LOGIN` and `SET PASSWORD` statements, the
/// secrets of `DuckDB`'s `CREATE SECRET` and SQL Server's `CREATE CREDENTIAL`, the keys and
/// passwords passed to `ClickHouse`'s table functions and engines (`s3`, `mysql`,
/// `postgresql`, `remote`…), and those of Redis's `AUTH`, `HELLO … AUTH`, `CONFIG SET
/// requirepass`/`masterauth` and `ACL SETUSER … >password`. Other statements are returned as
/// they are: a `WHERE password = 'x'` is data, not a credential. A literal it cannot tell the
/// end of is masked to the end of the statement.
#[must_use]
pub fn mask_secrets(sql: &str) -> Cow<'_, str> {
    match masked(sql) {
        Some((out, _)) if out != sql => Cow::Owned(out),
        _ => Cow::Borrowed(sql),
    }
}

/// The secrets [`mask_secrets`] hides from `sql`, as typed and with their escapes undone:
/// a server error about a statement can quote either form.
#[must_use]
pub fn secrets_of(sql: &str) -> Vec<String> {
    masked(sql).map(|(_, secrets)| secrets).unwrap_or_default()
}

/// `sql` masked and the secrets taken out of it; `None` when it names nothing a credential
/// could follow.
fn masked(sql: &str) -> Option<(String, Vec<String>)> {
    const HINTS: [&str; 16] = [
        "password",
        "identified",
        "secret",
        "auth",
        "requirepass",
        "setuser",
        "s3",
        "gcs",
        "oss",
        "cosn",
        "mysql",
        "postgresql",
        "mongodb",
        "remote",
        "azureblobstorage",
        "redis",
    ];
    let lower = sql.to_ascii_lowercase();
    if !HINTS.iter().any(|hint| lower.contains(hint)) {
        return None;
    }
    let (redis, mut secrets) = mask_redis(sql);
    let (out, more) = Masker::new(&redis).run();
    secrets.extend(more);
    Some((out, secrets))
}

/// `sql` with the passwords of the Redis commands that carry one masked, a command a line,
/// and the passwords taken out. A line that is not one of them is kept as it is.
fn mask_redis(sql: &str) -> (Cow<'_, str>, Vec<String>) {
    let mut secrets = Vec::new();
    let mut out = String::new();
    let mut changed = false;
    for line in sql.split_inclusive('\n') {
        let tokens = redis_tokens(line);
        let words: Vec<String> = tokens.iter().map(|t| t.value.clone()).collect();
        let mut found = redis_secrets(&words);
        found.sort_unstable();
        found.dedup();
        let mut at = 0;
        for (index, keep) in found {
            let Some(token) = tokens.get(index) else {
                continue;
            };
            out.push_str(&line[at..token.start]);
            let prefix: String = token.value.chars().take(keep).collect();
            let _ = write!(out, "{prefix}********");
            secrets.push(line[token.start..token.end].to_owned());
            secrets.push(token.value.clone());
            secrets.push(token.value.chars().skip(keep).collect());
            at = token.end;
            changed = true;
        }
        out.push_str(&line[at..]);
    }
    if changed {
        (Cow::Owned(out), secrets)
    } else {
        (Cow::Borrowed(sql), Vec::new())
    }
}

/// A word of a Redis command line: where it is, and its text with its quotes undone.
struct RedisToken {
    start: usize,
    end: usize,
    value: String,
}

/// The words of a Redis command line as `redis-cli` splits them: by blanks, a `"…"` word
/// with its backslash escapes, a `'…'` one as it is.
fn redis_tokens(line: &str) -> Vec<RedisToken> {
    let mut tokens = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some(&(start, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut value = String::new();
        let mut quote = None;
        let mut end = start;
        while let Some(&(at, c)) = chars.peek() {
            if quote.is_none() && c.is_whitespace() {
                break;
            }
            chars.next();
            end = at + c.len_utf8();
            match quote {
                None if c == '"' || c == '\'' => quote = Some(c),
                Some('"') if c == '\\' => {
                    if let Some((at, next)) = chars.next() {
                        value.push(next);
                        end = at + next.len_utf8();
                    }
                }
                Some(q) if c == q => quote = None,
                _ => value.push(c),
            }
        }
        tokens.push(RedisToken { start, end, value });
    }
    tokens
}

/// The words of a Redis command that are passwords, each with how many of its leading
/// characters are not (the `>` of `ACL SETUSER u >password`).
fn redis_secrets(words: &[String]) -> Vec<(usize, usize)> {
    let upper = |i: usize| words.get(i).map(|w| w.to_ascii_uppercase());
    let after = |keyword: &str, skip: usize| {
        (1..words.len())
            .find(|&i| words[i].eq_ignore_ascii_case(keyword))
            .map(|i| (i + skip, 0))
    };
    match upper(0).as_deref() {
        // `AUTH password` and `AUTH user password`.
        Some("AUTH") if words.len() > 1 => vec![(words.len() - 1, 0)],
        Some("HELLO") => after("AUTH", 2).into_iter().collect(),
        Some("MIGRATE") => after("AUTH", 1)
            .into_iter()
            .chain(after("AUTH2", 2))
            .collect(),
        Some("CONFIG") if upper(1).as_deref() == Some("SET") => (2..words.len())
            .step_by(2)
            .filter(|&i| {
                ["requirepass", "masterauth"]
                    .iter()
                    .any(|p| words[i].eq_ignore_ascii_case(p))
            })
            .map(|i| (i + 1, 0))
            .collect(),
        Some("ACL") if upper(1).as_deref() == Some("SETUSER") => (3..words.len())
            .filter(|&i| words[i].starts_with(['>', '<', '#']))
            .map(|i| (i, 1))
            .collect(),
        _ => Vec::new(),
    }
}

/// `text` with each of `secrets` replaced by `********`, for the error of a statement that
/// had a password: the server may quote the statement (`near 'hunter2' at line 1`).
#[must_use]
pub fn hide_secrets(text: &str, secrets: &[String]) -> String {
    let mut ordered: Vec<&String> = secrets.iter().filter(|s| !s.is_empty()).collect();
    // The longest first, so that a secret that holds another is hidden whole.
    ordered.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = text.to_owned();
    for secret in ordered {
        out = out.replace(secret.as_str(), "********");
    }
    out
}

/// Which arguments of a call are secrets, by their position from 0.
type SecretArguments = fn(usize) -> bool;

/// A call being read whose arguments can be credentials (`s3(url, key, secret)`).
struct Call {
    /// The parenthesis depth of its arguments.
    depth: usize,
    /// The argument being read.
    argument: usize,
    secret: SecretArguments,
}

/// Walks a script statement by statement. A statement is armed when it is one that sets
/// credentials; in an armed statement the literal after `PASSWORD`, `IDENTIFIED BY`,
/// `SECRET` and the like is the secret. In any statement the literal arguments of a call
/// to a `ClickHouse` table function or engine that takes credentials are, where they are
/// credentials.
struct Masker<'a> {
    sql: &'a str,
    out: String,
    /// The first words of the statement being read, upper case.
    words: Vec<String>,
    armed: bool,
    identified: bool,
    /// The next literal is a secret.
    secret_next: bool,
    /// The secrets read so far.
    secrets: Vec<String>,
    /// How deep in parentheses the masker is.
    depth: usize,
    /// The word just read names a call that takes credentials, if `(` follows.
    callee: Option<SecretArguments>,
    /// The calls with credentials being read, innermost last.
    calls: Vec<Call>,
}

impl<'a> Masker<'a> {
    fn new(sql: &'a str) -> Self {
        Self {
            sql,
            out: String::with_capacity(sql.len()),
            words: Vec::new(),
            armed: false,
            identified: false,
            secret_next: false,
            secrets: Vec::new(),
            depth: 0,
            callee: None,
            calls: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.words.clear();
        self.armed = false;
        self.identified = false;
        self.secret_next = false;
        self.depth = 0;
        self.calls.clear();
    }

    /// The call whose argument is read at this depth, if it takes credentials.
    fn call_here(&mut self) -> Option<&mut Call> {
        let depth = self.depth;
        self.calls.last_mut().filter(|call| call.depth == depth)
    }

    fn open(&mut self, callee: Option<SecretArguments>) {
        self.depth += 1;
        if let Some(secret) = callee {
            self.calls.push(Call {
                depth: self.depth,
                argument: 0,
                secret,
            });
        }
    }

    fn close(&mut self) {
        if self.call_here().is_some() {
            self.calls.pop();
        }
        self.depth = self.depth.saturating_sub(1);
    }

    fn run(mut self) -> (String, Vec<String>) {
        let b = self.sql.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let callee = self.callee.take();
            i = match b[i] {
                b';' => {
                    self.out.push(';');
                    self.reset();
                    i + 1
                }
                b'\'' => self.literal(i, i),
                // In MySQL a double-quoted token is a string literal, and a secret position
                // never takes an identifier, so there it is masked like a single-quoted one.
                b'"' if self.secret_next => self.literal(i, i),
                b'"' | b'`' => self.quoted(i, b[i]),
                b'[' => self.quoted(i, b']'),
                b'-' if b.get(i + 1) == Some(&b'-') => self.until(i, |c| c == b'\n'),
                b'/' if b.get(i + 1) == Some(&b'*') => self.block_comment(i),
                c if is_word(c) => self.word(i),
                b'=' => {
                    // `SET PASSWORD [FOR account] = 'secret'`.
                    self.secret_next |=
                        self.armed && self.words.first().is_some_and(|w| w == "SET");
                    self.copy_to(i, i + 1)
                }
                b'(' => {
                    self.open(callee);
                    self.copy_to(i, i + 1)
                }
                c if c.is_ascii_whitespace() => {
                    self.callee = callee;
                    self.copy_to(i, i + 1)
                }
                b')' => {
                    self.secret_next = false;
                    self.close();
                    self.copy_to(i, i + 1)
                }
                b',' => {
                    self.secret_next = false;
                    if let Some(call) = self.call_here() {
                        call.argument += 1;
                    }
                    self.copy_to(i, i + 1)
                }
                _ => {
                    self.secret_next = false;
                    // Copy the whole character: slicing inside a UTF-8 sequence panics.
                    let len = self.sql[i..].chars().next().map_or(1, char::len_utf8);
                    self.copy_to(i, i + len)
                }
            };
        }
        (self.out, self.secrets)
    }

    /// Copies `from..to`, and returns `to`.
    fn copy_to(&mut self, from: usize, to: usize) -> usize {
        self.out.push_str(&self.sql[from..to]);
        to
    }

    /// Copies up to, not including, the first byte `stop` accepts.
    fn until(&mut self, from: usize, stop: impl Fn(u8) -> bool) -> usize {
        let end = self.sql.as_bytes()[from..]
            .iter()
            .position(|&c| stop(c))
            .map_or(self.sql.len(), |p| from + p);
        self.copy_to(from, end)
    }

    fn block_comment(&mut self, from: usize) -> usize {
        let end = self.sql[from + 2..]
            .find("*/")
            .map_or(self.sql.len(), |p| from + 2 + p + 2);
        self.copy_to(from, end)
    }

    /// A quoted name starting at `from`, closing with `close`; a doubled close stays in it.
    fn quoted(&mut self, from: usize, close: u8) -> usize {
        self.secret_next = false;
        let b = self.sql.as_bytes();
        let mut i = from + 1;
        while i < b.len() {
            if b[i] == close {
                if b.get(i + 1) == Some(&close) {
                    i += 2;
                    continue;
                }
                i += 1;
                break;
            }
            i += 1;
        }
        self.copy_to(from, i)
    }

    /// A string literal whose opening quote is at `quote`, after a prefix starting at
    /// `from`. A backslash takes the next byte with it: unsure whether it escapes, this
    /// masks too much rather than too little.
    fn literal(&mut self, from: usize, quote: usize) -> usize {
        let b = self.sql.as_bytes();
        let close = b[quote];
        let mut i = quote + 1;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 2,
                c if c == close && b.get(i + 1) == Some(&close) => i += 2,
                c if c == close => {
                    i += 1;
                    break;
                }
                _ => i += 1,
            }
        }
        let mut end = i.min(b.len());
        // A backslash may have taken the first byte of a multi-byte character with it.
        while !self.sql.is_char_boundary(end) {
            end += 1;
        }
        let depth = self.depth;
        let in_call = self
            .calls
            .last()
            .is_some_and(|call| call.depth == depth && (call.secret)(call.argument));
        if self.secret_next || in_call {
            let terminated = end > quote + 1 && b[end - 1] == close && end - 1 > quote;
            let inner = &self.sql[quote + 1..if terminated { end - 1 } else { end }];
            self.secrets.push(inner.to_owned());
            self.secrets.push(unescape(inner, close));
            self.out.push_str(MASK);
            self.secret_next = false;
        } else {
            self.out.push_str(&self.sql[from..end]);
        }
        end
    }

    fn word(&mut self, from: usize) -> usize {
        let b = self.sql.as_bytes();
        let end = from
            + b[from..]
                .iter()
                .position(|&c| !is_word(c))
                .unwrap_or(b.len() - from);
        // `N'…'` and `E'…'` are literals with a prefix.
        if end == from + 1
            && matches!(b[from], b'N' | b'n' | b'E' | b'e')
            && b.get(end) == Some(&b'\'')
        {
            return self.literal(from, end);
        }
        let word = self.sql[from..end].to_ascii_uppercase();
        self.classify(&word);
        self.callee = credential_call(&word);
        self.copy_to(from, end)
    }

    fn classify(&mut self, word: &str) {
        let mut arming = false;
        if !self.armed && self.words.len() < 6 {
            self.words.push(word.to_owned());
            arming = is_credential_statement(&self.words);
            self.armed = arming;
        }
        // `SET PASSWORD FOR …` keeps its secret for the `=`.
        self.secret_next = false;
        // The `SECRET` of `CREATE SECRET name` says what is created, not what follows.
        if !self.armed || arming && word == "SECRET" {
            return;
        }
        match word {
            "PASSWORD"
            | "OLD_PASSWORD"
            | "SECRET"
            | "SESSION_TOKEN"
            | "TOKEN"
            | "BEARER_TOKEN"
            | "CLIENT_SECRET"
            | "CONNECTION_STRING"
            | "ACCOUNT_KEY"
            | "HTTP_PROXY_PASSWORD" => {
                self.secret_next = true;
            }
            "IDENTIFIED" => self.identified = true,
            "BY" | "AS" | "REPLACE" if self.identified => self.secret_next = true,
            _ => {}
        }
    }
}

/// The arguments that are credentials in a call to the `ClickHouse` table function or
/// table engine `word` (upper case), if it takes any. Where a position may hold either a
/// credential or something else, it is masked: the log loses a format name, never a key.
fn credential_call(word: &str) -> Option<SecretArguments> {
    Some(match word {
        // `s3(url, access_key_id, secret_access_key[, session_token], …)`.
        "S3" | "GCS" | "OSS" | "COSN" => |a| a == 2 || a == 3,
        "S3CLUSTER" => |a| a == 3 || a == 4,
        // `mysql(host, database, table, user, password, …)`, and the like.
        "MYSQL" | "POSTGRESQL" | "MONGODB" | "REDIS" | "MATERIALIZEDPOSTGRESQL" => |a| a == 4,
        // `remote(address, database, table, user, password)` or with `database.table`, one
        // argument less: the user may be masked with the password.
        "REMOTE" | "REMOTESECURE" => |a| a >= 3,
        // `azureBlobStorage(connection_string | url, container, blob, account, key, …)`.
        "AZUREBLOBSTORAGE" => |a| a == 0 || a == 4,
        "AZUREBLOBSTORAGECLUSTER" => |a| a == 1 || a == 5,
        _ => return None,
    })
}

/// `inner`, the text of a literal closed by `close`, as the server reads it: a doubled quote
/// is one, a backslash takes the next character as it is.
fn unescape(inner: &str, close: u8) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.extend(chars.next());
        } else if c == char::from(close) && chars.peek() == Some(&c) {
            chars.next();
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$' | b'@' | b'#')
}

/// Whether a statement that starts with `words` sets a credential: `GRANT`, `SET PASSWORD`,
/// and `CREATE`/`ALTER` of a user, role, login, secret (`DuckDB`'s `CREATE OR REPLACE
/// PERSISTENT SECRET`) or credential (SQL Server's `DATABASE SCOPED CREDENTIAL`).
fn is_credential_statement(words: &[String]) -> bool {
    let Some((first, rest)) = words.split_first() else {
        return false;
    };
    match first.as_str() {
        "GRANT" => true,
        "SET" => rest.first().is_some_and(|w| w == "PASSWORD"),
        "CREATE" | "ALTER" => {
            let mut rest = rest.iter().map(String::as_str).peekable();
            while let Some(word) = rest.next() {
                match word {
                    "OR" | "REPLACE" | "PERSISTENT" | "TEMPORARY" | "TEMP" => {}
                    "DATABASE" if rest.peek() == Some(&"SCOPED") => {
                        rest.next();
                    }
                    _ => {
                        return matches!(word, "USER" | "ROLE" | "LOGIN" | "SECRET" | "CREDENTIAL");
                    }
                }
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> PrincipalSpec {
        PrincipalSpec {
            name: name.to_owned(),
            ..PrincipalSpec::default()
        }
    }

    fn one(d: Dialect, change: &SecurityChange) -> String {
        let all = statements(d, change, false).unwrap();
        assert_eq!(all.len(), 1, "{all:?}");
        all.into_iter().next().unwrap()
    }

    fn grantee(name: &str) -> Grantee {
        Grantee {
            name: name.to_owned(),
            host: None,
        }
    }

    fn grant(target: GrantTarget, privileges: &[&str]) -> SecurityChange {
        SecurityChange::Privileges {
            action: GrantAction::Grant,
            grantees: vec![grantee("app")],
            target,
            privileges: privileges.iter().map(|p| (*p).to_owned()).collect(),
            grant_option: false,
            cascade: false,
        }
    }

    fn table(schema: &str, name: &str) -> GrantTarget {
        GrantTarget::Table {
            schema: schema.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn creates_a_postgres_user_with_attributes() {
        let mut s = spec("App User");
        s.flags_on = vec![PrincipalFlag::CreateDb];
        s.flags_off = vec![PrincipalFlag::Inherit];
        s.connection_limit = Some(5);
        s.password = Some("it's".to_owned());
        s.valid_until = Some("2030-01-01".to_owned());
        let change = SecurityChange::Create { spec: s };
        assert_eq!(
            one(Dialect::Postgres, &change),
            "CREATE USER \"App User\" WITH CREATEDB NOINHERIT CONNECTION LIMIT 5 \
             PASSWORD 'it''s' VALID UNTIL '2030-01-01'"
        );
        let masked = statements(Dialect::Postgres, &change, true).unwrap();
        assert!(masked[0].contains("PASSWORD '********'") && !masked[0].contains("it's"));
    }

    #[test]
    fn creates_a_postgres_role_and_a_user_that_cannot_log_in() {
        let mut role = spec("readers");
        role.kind = PrincipalKind::Role;
        assert_eq!(
            one(
                Dialect::Postgres,
                &SecurityChange::Create { spec: role.clone() }
            ),
            "CREATE ROLE readers"
        );
        role.flags_on = vec![PrincipalFlag::Login];
        assert_eq!(
            one(Dialect::Postgres, &SecurityChange::Create { spec: role }),
            "CREATE ROLE readers WITH LOGIN"
        );
        let mut user = spec("u");
        user.flags_off = vec![PrincipalFlag::Login];
        assert_eq!(
            one(Dialect::Postgres, &SecurityChange::Create { spec: user }),
            "CREATE USER u WITH NOLOGIN"
        );
    }

    #[test]
    fn a_postgres_password_with_a_backslash_is_an_escape_string() {
        let mut s = spec("u");
        s.password = Some(r"a\b".to_owned());
        let sql = one(Dialect::Postgres, &SecurityChange::Create { spec: s });
        assert!(sql.ends_with(r"PASSWORD E'a\\b'"), "{sql}");
    }

    #[test]
    fn alters_postgres_only_what_changes_and_renames_last() {
        let mut s = spec("app");
        s.flags_on = vec![PrincipalFlag::Superuser];
        s.flags_off = vec![PrincipalFlag::Login];
        s.connection_limit = Some(-1);
        s.valid_until = Some(String::new());
        s.rename_to = Some("web".to_owned());
        let all = statements(Dialect::Postgres, &SecurityChange::Alter { spec: s }, false).unwrap();
        assert_eq!(
            all,
            [
                "ALTER ROLE app WITH NOLOGIN SUPERUSER CONNECTION LIMIT -1 VALID UNTIL 'infinity'",
                "ALTER ROLE app RENAME TO web"
            ]
        );
        assert_eq!(
            statements(
                Dialect::Postgres,
                &SecurityChange::Alter { spec: spec("app") },
                false
            ),
            Err("nothing to change".to_owned())
        );
    }

    #[test]
    fn drops_a_postgres_role_after_handing_over_what_it_owns() {
        let change = SecurityChange::Drop {
            name: "app".to_owned(),
            host: None,
            kind: PrincipalKind::User,
            options: DropOptions {
                reassign_to: Some("postgres".to_owned()),
                drop_owned: true,
                drop_login: false,
                login: None,
            },
        };
        assert_eq!(
            statements(Dialect::Postgres, &change, false).unwrap(),
            [
                "REASSIGN OWNED BY app TO postgres",
                "DROP OWNED BY app",
                "DROP ROLE app"
            ]
        );
    }

    #[test]
    fn writes_mysql_accounts_with_their_host() {
        let mut s = spec("app");
        s.host = Some("10.%".to_owned());
        s.password = Some("p'w".to_owned());
        s.connection_limit = Some(3);
        s.flags_on = vec![PrincipalFlag::Locked, PrincipalFlag::PasswordExpired];
        let create = SecurityChange::Create { spec: s.clone() };
        assert_eq!(
            one(Dialect::MySql, &create),
            "CREATE USER 'app'@'10.%' IDENTIFIED BY 'p''w' WITH MAX_USER_CONNECTIONS 3 \
             ACCOUNT LOCK PASSWORD EXPIRE"
        );
        s.flags_on.clear();
        s.flags_off = vec![PrincipalFlag::Locked];
        s.rename_to = Some("web".to_owned());
        s.password = None;
        s.connection_limit = None;
        assert_eq!(
            statements(Dialect::MySql, &SecurityChange::Alter { spec: s }, false).unwrap(),
            [
                "ALTER USER 'app'@'10.%' ACCOUNT UNLOCK",
                "RENAME USER 'app'@'10.%' TO 'web'@'10.%'"
            ]
        );
    }

    #[test]
    fn mysql_roles_and_drops() {
        let mut role = spec("readers");
        role.kind = PrincipalKind::Role;
        assert_eq!(
            one(Dialect::MySql, &SecurityChange::Create { spec: role }),
            "CREATE ROLE 'readers'"
        );
        let drop = SecurityChange::Drop {
            name: "app".to_owned(),
            host: Some("%".to_owned()),
            kind: PrincipalKind::User,
            options: DropOptions::default(),
        };
        assert_eq!(one(Dialect::MySql, &drop), "DROP USER 'app'@'%'");
        let mut expiry = spec("app");
        expiry.valid_until = Some("2030-01-01".to_owned());
        assert!(
            statements(
                Dialect::MySql,
                &SecurityChange::Create { spec: expiry },
                false
            )
            .is_err()
        );
    }

    #[test]
    fn sql_server_creates_a_login_and_its_user() {
        let mut s = spec("app");
        s.password = Some("Pa'ss".to_owned());
        s.flags_on = vec![PrincipalFlag::Locked];
        let change = SecurityChange::Create { spec: s.clone() };
        assert_eq!(
            statements(Dialect::SqlServer, &change, false).unwrap(),
            [
                "CREATE LOGIN app WITH PASSWORD = N'Pa''ss'",
                "CREATE USER app FOR LOGIN app",
                "ALTER LOGIN app DISABLE"
            ]
        );
        s.password = None;
        s.flags_on.clear();
        assert_eq!(
            one(Dialect::SqlServer, &SecurityChange::Create { spec: s }),
            "CREATE USER app WITHOUT LOGIN"
        );
        let drop = SecurityChange::Drop {
            name: "app".to_owned(),
            host: None,
            kind: PrincipalKind::User,
            options: DropOptions {
                drop_login: true,
                login: Some("app".to_owned()),
                ..DropOptions::default()
            },
        };
        assert_eq!(
            statements(Dialect::SqlServer, &drop, false).unwrap(),
            ["DROP USER app", "DROP LOGIN app"]
        );
    }

    #[test]
    fn sql_server_changes_the_login_by_its_own_name() {
        let mut s = spec("app");
        s.login = Some("corp\\app".to_owned());
        s.password = Some("p".to_owned());
        s.flags_on = vec![PrincipalFlag::Locked];
        s.rename_to = Some("app2".to_owned());
        assert_eq!(
            statements(
                Dialect::SqlServer,
                &SecurityChange::Alter { spec: s.clone() },
                false
            )
            .unwrap(),
            [
                "ALTER LOGIN [corp\\app] WITH PASSWORD = N'p'",
                "ALTER LOGIN [corp\\app] DISABLE",
                "ALTER USER app WITH NAME = app2"
            ]
        );
        s.flags_on.clear();
        s.flags_off = vec![PrincipalFlag::Locked];
        s.password = None;
        s.rename_to = None;
        assert_eq!(
            one(
                Dialect::SqlServer,
                &SecurityChange::Alter { spec: s.clone() }
            ),
            "ALTER LOGIN [corp\\app] ENABLE"
        );
        // No mapped login: no login-level change, but a rename still works.
        s.login = None;
        assert!(
            statements(
                Dialect::SqlServer,
                &SecurityChange::Alter { spec: s.clone() },
                false
            )
            .is_err()
        );
        s.flags_off.clear();
        s.rename_to = Some("app2".to_owned());
        assert_eq!(
            one(Dialect::SqlServer, &SecurityChange::Alter { spec: s }),
            "ALTER USER app WITH NAME = app2"
        );
        let drop = |login: Option<&str>| SecurityChange::Drop {
            name: "app".to_owned(),
            host: None,
            kind: PrincipalKind::User,
            options: DropOptions {
                drop_login: true,
                login: login.map(str::to_owned),
                ..DropOptions::default()
            },
        };
        assert_eq!(
            statements(Dialect::SqlServer, &drop(Some("lg")), false).unwrap(),
            ["DROP USER app", "DROP LOGIN lg"]
        );
        assert_eq!(one(Dialect::SqlServer, &drop(None)), "DROP USER app");
    }

    #[test]
    fn clickhouse_users_are_not_identified_without_a_password() {
        let s = spec("app");
        assert_eq!(
            one(
                Dialect::ClickHouse,
                &SecurityChange::Create { spec: s.clone() }
            ),
            "CREATE USER app NOT IDENTIFIED"
        );
        let mut s = s;
        s.password = Some("pw".to_owned());
        assert_eq!(
            one(Dialect::ClickHouse, &SecurityChange::Create { spec: s }),
            "CREATE USER app IDENTIFIED WITH sha256_password BY 'pw'"
        );
        let mut limit = spec("app");
        limit.connection_limit = Some(1);
        assert!(
            statements(
                Dialect::ClickHouse,
                &SecurityChange::Create { spec: limit },
                false
            )
            .is_err()
        );
    }

    #[test]
    fn a_flag_the_database_lacks_is_refused() {
        let mut s = spec("app");
        s.flags_on = vec![PrincipalFlag::Superuser];
        assert!(statements(Dialect::MySql, &SecurityChange::Create { spec: s }, false).is_err());
        assert!(
            statements(
                Dialect::Sqlite,
                &SecurityChange::Create { spec: spec("a") },
                false
            )
            .is_err()
        );
        assert!(
            statements(
                Dialect::Postgres,
                &SecurityChange::Create { spec: spec(" ") },
                false
            )
            .is_err()
        );
    }

    #[test]
    fn grants_on_tables_in_every_dialect() {
        let change = grant(table("public", "Orders"), &["select", "insert"]);
        assert_eq!(
            one(Dialect::Postgres, &change),
            "GRANT SELECT, INSERT ON TABLE public.\"Orders\" TO app"
        );
        assert_eq!(
            one(Dialect::MySql, &change),
            "GRANT SELECT, INSERT ON public.Orders TO 'app'"
        );
        assert_eq!(
            one(Dialect::SqlServer, &change),
            "GRANT SELECT, INSERT ON OBJECT::[public].Orders TO app"
        );
        assert_eq!(
            one(Dialect::ClickHouse, &change),
            "GRANT SELECT, INSERT ON public.Orders TO app"
        );
    }

    #[test]
    fn grants_with_the_grant_option_and_to_a_mysql_account() {
        let change = SecurityChange::Privileges {
            action: GrantAction::Grant,
            grantees: vec![Grantee {
                name: "app".to_owned(),
                host: Some("%".to_owned()),
            }],
            target: GrantTarget::Server,
            privileges: vec!["ALL".to_owned(), "SELECT".to_owned()],
            grant_option: true,
            cascade: false,
        };
        assert_eq!(
            one(Dialect::MySql, &change),
            "GRANT ALL ON *.* TO 'app'@'%' WITH GRANT OPTION"
        );
        assert!(statements(Dialect::Postgres, &change, false).is_err());
    }

    #[test]
    fn schema_and_database_scopes() {
        let schema = GrantTarget::Schema {
            name: "sales".to_owned(),
        };
        assert_eq!(
            one(Dialect::Postgres, &grant(schema.clone(), &["USAGE"])),
            "GRANT USAGE ON SCHEMA sales TO app"
        );
        assert_eq!(
            one(Dialect::SqlServer, &grant(schema, &["SELECT"])),
            "GRANT SELECT ON SCHEMA::sales TO app"
        );
        let db = GrantTarget::Database {
            name: "shop".to_owned(),
        };
        assert_eq!(
            one(Dialect::Postgres, &grant(db.clone(), &["CONNECT"])),
            "GRANT CONNECT ON DATABASE shop TO app"
        );
        assert_eq!(
            one(Dialect::MySql, &grant(db.clone(), &["SELECT"])),
            "GRANT SELECT ON shop.* TO 'app'"
        );
        assert_eq!(
            one(Dialect::SqlServer, &grant(db, &["CONNECT"])),
            "GRANT CONNECT TO app"
        );
        let all = GrantTarget::AllTables {
            schema: "public".to_owned(),
        };
        assert_eq!(
            one(Dialect::Postgres, &grant(all, &["SELECT"])),
            "GRANT SELECT ON ALL TABLES IN SCHEMA public TO app"
        );
        let routines = GrantTarget::AllRoutines {
            schema: "public".to_owned(),
        };
        assert_eq!(
            one(Dialect::Postgres, &grant(routines.clone(), &["EXECUTE"])),
            "GRANT EXECUTE ON ALL ROUTINES IN SCHEMA public TO app"
        );
        assert!(statements(Dialect::ClickHouse, &grant(routines, &["EXECUTE"]), false).is_err());
    }

    #[test]
    fn refuses_a_privilege_the_object_cannot_have() {
        assert!(
            statements(
                Dialect::Postgres,
                &grant(table("public", "t"), &["USAGE"]),
                false
            )
            .is_err()
        );
        assert!(
            statements(
                Dialect::Postgres,
                &grant(table("public", "t"), &["SELECT; DROP TABLE t"]),
                false
            )
            .is_err()
        );
        assert!(statements(Dialect::Postgres, &grant(table("public", "t"), &[]), false).is_err());
    }

    #[test]
    fn sql_server_all_on_a_table_lists_what_it_means() {
        assert_eq!(
            one(Dialect::SqlServer, &grant(table("dbo", "t"), &["ALL"])),
            "GRANT SELECT, INSERT, UPDATE, DELETE, REFERENCES ON OBJECT::dbo.t TO app"
        );
    }

    #[test]
    fn revokes() {
        let revoke = |option: bool, cascade: bool| SecurityChange::Privileges {
            action: GrantAction::Revoke,
            grantees: vec![grantee("app")],
            target: table("public", "t"),
            privileges: vec!["SELECT".to_owned()],
            grant_option: option,
            cascade,
        };
        assert_eq!(
            one(Dialect::Postgres, &revoke(false, true)),
            "REVOKE SELECT ON TABLE public.t FROM app CASCADE"
        );
        assert_eq!(
            one(Dialect::Postgres, &revoke(true, false)),
            "REVOKE GRANT OPTION FOR SELECT ON TABLE public.t FROM app"
        );
        assert_eq!(
            one(Dialect::MySql, &revoke(true, false)),
            "REVOKE GRANT OPTION ON public.t FROM 'app'"
        );
        assert_eq!(
            one(Dialect::MySql, &revoke(false, true)),
            "REVOKE SELECT ON public.t FROM 'app'"
        );
        assert!(statements(Dialect::ClickHouse, &revoke(true, false), false).is_err());
    }

    #[test]
    fn grants_and_revokes_role_membership() {
        let membership = |action, admin| SecurityChange::Membership {
            action,
            role: "readers".to_owned(),
            members: vec![grantee("app"), grantee("web")],
            admin_option: admin,
        };
        assert_eq!(
            one(Dialect::Postgres, &membership(GrantAction::Grant, true)),
            "GRANT readers TO app, web WITH ADMIN OPTION"
        );
        assert_eq!(
            one(Dialect::Postgres, &membership(GrantAction::Revoke, true)),
            "REVOKE ADMIN OPTION FOR readers FROM app, web"
        );
        assert_eq!(
            one(Dialect::MySql, &membership(GrantAction::Revoke, false)),
            "REVOKE 'readers' FROM 'app', 'web'"
        );
        assert!(
            statements(
                Dialect::MySql,
                &membership(GrantAction::Revoke, true),
                false
            )
            .is_err()
        );
        assert_eq!(
            statements(
                Dialect::SqlServer,
                &membership(GrantAction::Grant, false),
                false
            )
            .unwrap(),
            [
                "ALTER ROLE readers ADD MEMBER app",
                "ALTER ROLE readers ADD MEMBER web"
            ]
        );
        assert_eq!(
            one(Dialect::ClickHouse, &membership(GrantAction::Grant, false)),
            "GRANT readers TO app, web"
        );
    }

    #[test]
    fn privilege_lists_have_all_where_the_database_has_it() {
        assert!(privileges(Dialect::Postgres, TargetClass::Table).contains(&"ALL"));
        assert_eq!(
            privileges(Dialect::Postgres, TargetClass::Server),
            &[] as &[&str]
        );
        assert!(!privileges(Dialect::SqlServer, TargetClass::Schema).contains(&"ALL"));
        assert_eq!(
            privileges(Dialect::Sqlite, TargetClass::Table),
            &[] as &[&str]
        );
    }

    #[test]
    fn masks_passwords_of_credential_statements() {
        for (sql, expected) in [
            (
                "CREATE USER app WITH PASSWORD 'hunter2' VALID UNTIL '2030-01-01'",
                "CREATE USER app WITH PASSWORD '********' VALID UNTIL '2030-01-01'",
            ),
            (
                "ALTER ROLE \"a\" PASSWORD E'x\\\\y'",
                "ALTER ROLE \"a\" PASSWORD '********'",
            ),
            (
                "CREATE USER 'a'@'%' IDENTIFIED BY 'it''s' WITH MAX_USER_CONNECTIONS 3",
                "CREATE USER 'a'@'%' IDENTIFIED BY '********' WITH MAX_USER_CONNECTIONS 3",
            ),
            (
                "CREATE LOGIN [a] WITH PASSWORD = N'p;w', DEFAULT_DATABASE = [x]",
                "CREATE LOGIN [a] WITH PASSWORD = '********', DEFAULT_DATABASE = [x]",
            ),
            (
                "ALTER LOGIN a WITH PASSWORD = 'new' OLD_PASSWORD = 'old'",
                "ALTER LOGIN a WITH PASSWORD = '********' OLD_PASSWORD = '********'",
            ),
            (
                "CREATE USER a IDENTIFIED WITH sha256_password BY 'pw' VALID UNTIL '2030-01-01'",
                "CREATE USER a IDENTIFIED WITH sha256_password BY '********' VALID UNTIL '2030-01-01'",
            ),
            (
                "SET PASSWORD FOR 'a'@'%' = 'pw'",
                "SET PASSWORD FOR 'a'@'%' = '********'",
            ),
            (
                "CREATE USER a IDENTIFIED BY 'one'; CREATE ROLE r; ALTER USER b IDENTIFIED BY 'two'",
                "CREATE USER a IDENTIFIED BY '********'; CREATE ROLE r; ALTER USER b IDENTIFIED BY '********'",
            ),
        ] {
            assert_eq!(mask_secrets(sql), expected, "{sql}");
        }
    }

    #[test]
    fn masks_redis_passwords() {
        for (sql, expected) in [
            ("AUTH hunter2", "AUTH ********"),
            ("auth default \"hun ter2\"", "auth default ********"),
            (
                "HELLO 3 AUTH default hunter2 SETNAME app",
                "HELLO 3 AUTH default ******** SETNAME app",
            ),
            (
                "CONFIG SET requirepass hunter2",
                "CONFIG SET requirepass ********",
            ),
            (
                "config set maxmemory 1gb masterauth 'pw' masteruser m",
                "config set maxmemory 1gb masterauth ******** masteruser m",
            ),
            (
                "ACL SETUSER app on >hunter2 <old #0a1b ~* +@all",
                "ACL SETUSER app on >******** <******** #******** ~* +@all",
            ),
            (
                "MIGRATE h 6379 k 0 5000 AUTH2 u pw KEYS a",
                "MIGRATE h 6379 k 0 5000 AUTH2 u ******** KEYS a",
            ),
            ("GET k\nAUTH pw\nGET j", "GET k\nAUTH ********\nGET j"),
        ] {
            assert_eq!(mask_secrets(sql), expected, "{sql}");
        }
        let secrets = secrets_of("ACL SETUSER app >hunter2");
        assert!(secrets.contains(&"hunter2".to_owned()), "{secrets:?}");
        assert!(secrets.contains(&">hunter2".to_owned()), "{secrets:?}");
        // Commands that carry no password, and a password missing, are kept.
        for sql in [
            "GET auth",
            "AUTH",
            "CONFIG SET maxmemory 1gb",
            "ACL SETUSER app on",
        ] {
            assert_eq!(mask_secrets(sql), sql, "{sql}");
        }
    }

    #[test]
    fn masks_duckdb_secrets_and_sql_server_credentials() {
        for (sql, expected) in [
            (
                "CREATE SECRET (TYPE s3, KEY_ID 'AKIA', SECRET 'x/y', REGION 'eu')",
                "CREATE SECRET (TYPE s3, KEY_ID 'AKIA', SECRET '********', REGION 'eu')",
            ),
            (
                "CREATE OR REPLACE PERSISTENT SECRET s3s (TYPE s3, SECRET 'a', SESSION_TOKEN 'b')",
                "CREATE OR REPLACE PERSISTENT SECRET s3s (TYPE s3, SECRET '********', SESSION_TOKEN '********')",
            ),
            (
                "CREATE SECRET hf (TYPE huggingface, TOKEN 'hf_x'); CREATE SECRET \"s\" (TYPE azure, CONNECTION_STRING 'k=v')",
                "CREATE SECRET hf (TYPE huggingface, TOKEN '********'); CREATE SECRET \"s\" (TYPE azure, CONNECTION_STRING '********')",
            ),
            (
                "CREATE CREDENTIAL c WITH IDENTITY = 'app', SECRET = 'pw'",
                "CREATE CREDENTIAL c WITH IDENTITY = 'app', SECRET = '********'",
            ),
            (
                "ALTER DATABASE SCOPED CREDENTIAL c WITH IDENTITY = 'app', SECRET = N'pw'",
                "ALTER DATABASE SCOPED CREDENTIAL c WITH IDENTITY = 'app', SECRET = '********'",
            ),
        ] {
            assert_eq!(mask_secrets(sql), expected, "{sql}");
        }
        for sql in [
            "SELECT secret FROM t WHERE secret = 'x'",
            "CREATE TABLE secret (token text DEFAULT 'x')",
            "CREATE DATABASE user WITH OWNER = 'x'",
        ] {
            assert_eq!(mask_secrets(sql), sql, "{sql}");
        }
    }

    #[test]
    fn masks_clickhouse_table_function_credentials() {
        for (sql, expected) in [
            (
                "SELECT * FROM s3('https://b/k.csv', 'AKIA', 'sec', 'CSV')",
                "SELECT * FROM s3('https://b/k.csv', 'AKIA', '********', '********')",
            ),
            (
                "INSERT INTO FUNCTION mysql('h:3306', 'db', 't', 'u', 'pw') SELECT 1",
                "INSERT INTO FUNCTION mysql('h:3306', 'db', 't', 'u', '********') SELECT 1",
            ),
            (
                "SELECT * FROM postgresql('h:5432', 'db', 't', 'u', 'p''w', 'public')",
                "SELECT * FROM postgresql('h:5432', 'db', 't', 'u', '********', 'public')",
            ),
            (
                "SELECT * FROM remote('h:9000', db.t, 'u', 'pw')",
                "SELECT * FROM remote('h:9000', db.t, 'u', '********')",
            ),
            (
                "SELECT * FROM remoteSecure('h', 'db', 't', 'u', 'pw')",
                "SELECT * FROM remoteSecure('h', 'db', 't', '********', '********')",
            ),
            (
                "CREATE TABLE t (a Int8) ENGINE = MySQL('h:3306', 'db', 't', 'u', 'pw')",
                "CREATE TABLE t (a Int8) ENGINE = MySQL('h:3306', 'db', 't', 'u', '********')",
            ),
            // A nested call's literals are its own: only the password is the function's.
            (
                "SELECT * FROM s3(concat('https://', 'b'), 'k', 's')",
                "SELECT * FROM s3(concat('https://', 'b'), 'k', '********')",
            ),
        ] {
            assert_eq!(mask_secrets(sql), expected, "{sql}");
        }
        let secrets = secrets_of("SELECT * FROM mysql('h', 'db', 't', 'u', 'hunter2')");
        assert!(secrets.contains(&"hunter2".to_owned()), "{secrets:?}");
        for sql in [
            "SELECT * FROM t CROSS JOIN u WHERE a = 'x'",
            "SELECT * FROM mysql.user WHERE user = 'x'",
            "SELECT s3, 'x' FROM t",
        ] {
            assert_eq!(mask_secrets(sql), sql, "{sql}");
        }
    }

    #[test]
    fn leaves_other_statements_alone() {
        for sql in [
            "SELECT * FROM users WHERE password = 'abc'",
            "UPDATE users SET password = 'abc' WHERE id = 1",
            "CREATE TABLE t (password text DEFAULT 'x')",
            "CREATE USER a",
            "SELECT 1",
        ] {
            assert_eq!(mask_secrets(sql), sql, "{sql}");
        }
        assert!(matches!(mask_secrets("SELECT 1"), Cow::Borrowed(_)));
    }

    fn info(product: &str, version: &str) -> ServerInfo {
        ServerInfo {
            product: product.to_owned(),
            version: version.to_owned(),
            encrypted: false,
        }
    }

    #[test]
    fn secrets_are_found_and_hidden_in_an_error_text() {
        let sql = "CREATE USER a IDENTIFIED BY 'hun''ter2'; ALTER USER b PASSWORD \"é日\"";
        let secrets = secrets_of(sql);
        assert!(secrets.contains(&"hun''ter2".to_owned()));
        assert!(secrets.contains(&"hun'ter2".to_owned()));
        assert!(secrets.contains(&"é日".to_owned()));
        assert_eq!(
            hide_secrets(
                "You have an error near 'hun'ter2' at line 1; and \"é日\"",
                &secrets
            ),
            "You have an error near '********' at line 1; and \"********\""
        );
        let none: Vec<String> = Vec::new();
        assert_eq!(secrets_of("SELECT password FROM t WHERE x = 'y'"), none);
        assert_eq!(hide_secrets("nothing", &[String::new()]), "nothing");
    }

    #[test]
    fn mysql_before_8_has_no_roles() {
        assert!(!roles_supported(
            Dialect::MySql,
            &info("MySQL", "5.7.44-log")
        ));
        assert!(roles_supported(Dialect::MySql, &info("MySQL", "8.0.36")));
        assert!(roles_supported(
            Dialect::MySql,
            &info("MariaDB", "10.6.21-MariaDB")
        ));
        assert!(roles_supported(
            Dialect::MySql,
            &info("MySQL", "5.7.25-TiDB-v7.5.0")
        ));
        assert!(roles_supported(
            Dialect::Postgres,
            &info("PostgreSQL", "16.4")
        ));
        let mut role = spec("r");
        role.kind = PrincipalKind::Role;
        assert!(involves_roles(&SecurityChange::Create { spec: role }));
        assert!(!involves_roles(&SecurityChange::Create { spec: spec("u") }));
        assert!(involves_roles(&SecurityChange::Membership {
            action: GrantAction::Grant,
            role: "r".to_owned(),
            members: vec![],
            admin_option: false,
        }));
    }

    #[test]
    fn non_ascii_text_never_panics_the_masker() {
        for sql in [
            "SELECT password FROM utenti WHERE città = 1",
            "SELECT password FROM \"città\" WHERE nome = 'perché'",
            "SELECT password -- è così\nFROM t",
            "SELECT password /* 日本語 */ FROM `表`",
            "SELECT password FROM [名前] WHERE x = N'日本'",
            "SELECT password, 'a\\è' FROM t",
        ] {
            assert_eq!(mask_secrets(sql), sql, "{sql}");
        }
        assert_eq!(
            mask_secrets("ALTER USER città PASSWORD 'pässwörd日本'; SELECT è"),
            "ALTER USER città PASSWORD '********'; SELECT è"
        );
        assert_eq!(
            mask_secrets("CREATE USER 'é' IDENTIFIED BY 'x\\é'"),
            "CREATE USER 'é' IDENTIFIED BY '********'"
        );
    }

    #[test]
    fn random_mixes_of_ascii_and_non_ascii_never_panic() {
        let pieces = [
            "password",
            " ",
            "'",
            "\"",
            "`",
            "[",
            "]",
            "\\",
            "--",
            "/*",
            "*/",
            ";",
            "=",
            "(",
            "è",
            "日",
            "😀",
            "IDENTIFIED BY",
            "SET",
            "ALTER USER",
            "\n",
        ];
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        for _ in 0..3000 {
            let mut sql = String::from("password ");
            for _ in 0..12 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                sql.push_str(pieces[usize::try_from(seed % 21).unwrap() % pieces.len()]);
            }
            let _ = mask_secrets(&sql);
        }
    }

    #[test]
    fn masks_double_quoted_secrets_as_mysql_reads_them() {
        assert_eq!(
            mask_secrets("CREATE USER a IDENTIFIED BY \"hunter2\""),
            "CREATE USER a IDENTIFIED BY '********'"
        );
        assert_eq!(
            mask_secrets("SET PASSWORD FOR a = \"hunter2\""),
            "SET PASSWORD FOR a = '********'"
        );
        assert_eq!(
            mask_secrets("ALTER USER a IDENTIFIED BY \"hun\"\"ter;2\"; SELECT 1"),
            "ALTER USER a IDENTIFIED BY '********'; SELECT 1"
        );
        // An identifier outside a secret position stays as it is.
        assert_eq!(
            mask_secrets("ALTER USER \"a\" PASSWORD 'x'"),
            "ALTER USER \"a\" PASSWORD '********'"
        );
    }

    #[test]
    fn a_masked_preview_never_holds_the_password() {
        let mut s = spec("app");
        s.password = Some("S3cret!".to_owned());
        s.connection_limit = Some(2);
        s.login = Some("app".to_owned());
        for d in [
            Dialect::Postgres,
            Dialect::MySql,
            Dialect::SqlServer,
            Dialect::ClickHouse,
        ] {
            if d == Dialect::SqlServer || d == Dialect::MySql || d == Dialect::Postgres {
                s.connection_limit = (d != Dialect::SqlServer).then_some(2);
            } else {
                s.connection_limit = None;
            }
            for change in [
                SecurityChange::Create { spec: s.clone() },
                SecurityChange::Alter { spec: s.clone() },
            ] {
                let shown = statements(d, &change, true).unwrap().join(";\n");
                assert!(!shown.contains("S3cret!"), "{d:?}: {shown}");
                let run = statements(d, &change, false).unwrap().join(";\n");
                // What runs is masked the same way by the log.
                assert!(!mask_secrets(&run).contains("S3cret!"), "{d:?}: {run}");
            }
        }
    }
}

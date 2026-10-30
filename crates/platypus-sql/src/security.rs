//! The statements that create, change and drop users and roles and that grant and revoke
//! privileges, for PostgreSQL, MySQL and `MariaDB`, SQL Server and `ClickHouse`
//! ([`statements`]); and [`mask_secrets`], which hides passwords from the query log.
//!
//! The app shows the statements before they run and writes them to its query log; a
//! password is in the statement that runs, and nowhere else.

use std::borrow::Cow;

use platypus_core::{
    DropOptions, GrantAction, GrantTarget, Grantee, PrincipalFlag, PrincipalKind, PrincipalSpec,
    SecurityChange, TargetClass,
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
                if options.drop_login && kind == PrincipalKind::User {
                    out.push(format!("DROP LOGIN {who}"));
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
        if let Some(p) = non_empty(spec.password.as_deref()) {
            out.push(format!(
                "ALTER LOGIN {name} WITH PASSWORD = {}",
                self.password_literal(p)
            ));
        }
        if spec.flags_on.contains(&PrincipalFlag::Locked) {
            out.push(format!("ALTER LOGIN {name} DISABLE"));
        } else if spec.flags_off.contains(&PrincipalFlag::Locked) {
            out.push(format!("ALTER LOGIN {name} ENABLE"));
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

/// `sql` with the passwords of its `CREATE`/`ALTER` `USER`/`ROLE`/`LOGIN` and
/// `SET PASSWORD` statements replaced by `'********'`, so that it can go to a log. Other
/// statements are returned as they are: a `WHERE password = 'x'` is data, not a
/// credential. A literal it cannot tell the end of is masked to the end of the statement.
#[must_use]
pub fn mask_secrets(sql: &str) -> Cow<'_, str> {
    let lower = sql.to_ascii_lowercase();
    if !lower.contains("password") && !lower.contains("identified") {
        return Cow::Borrowed(sql);
    }
    Cow::Owned(Masker::new(sql).run())
}

/// Walks a script statement by statement. A statement is armed when it is one that sets
/// credentials; in an armed statement the literal after `PASSWORD`, `IDENTIFIED BY` and
/// the like is the secret.
struct Masker<'a> {
    sql: &'a str,
    out: String,
    /// The first words of the statement being read, upper case.
    words: Vec<String>,
    armed: bool,
    identified: bool,
    /// The next literal is a secret.
    secret_next: bool,
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
        }
    }

    fn reset(&mut self) {
        self.words.clear();
        self.armed = false;
        self.identified = false;
        self.secret_next = false;
    }

    fn run(mut self) -> String {
        let b = self.sql.as_bytes();
        let mut i = 0;
        while i < b.len() {
            i = match b[i] {
                b';' => {
                    self.out.push(';');
                    self.reset();
                    i + 1
                }
                b'\'' => self.literal(i, i),
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
                b'(' => self.copy_to(i, i + 1),
                c if c.is_ascii_whitespace() => self.copy_to(i, i + 1),
                _ => {
                    self.secret_next = false;
                    self.copy_to(i, i + 1)
                }
            };
        }
        self.out
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
        let mut i = quote + 1;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 2,
                b'\'' if b.get(i + 1) == Some(&b'\'') => i += 2,
                b'\'' => {
                    i += 1;
                    break;
                }
                _ => i += 1,
            }
        }
        let end = i.min(b.len());
        if self.secret_next {
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
        self.copy_to(from, end)
    }

    fn classify(&mut self, word: &str) {
        if self.words.len() < 2 {
            self.words.push(word.to_owned());
            self.armed = self.armed || is_credential_statement(&self.words);
        }
        // `SET PASSWORD FOR …` keeps its secret for the `=`.
        self.secret_next = false;
        if !self.armed {
            return;
        }
        match word {
            "PASSWORD" | "OLD_PASSWORD" => self.secret_next = true,
            "IDENTIFIED" => self.identified = true,
            "BY" | "AS" | "REPLACE" if self.identified => self.secret_next = true,
            _ => {}
        }
    }
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'$' | b'@' | b'#')
}

/// Whether a statement that starts with `words` sets a credential.
fn is_credential_statement(words: &[String]) -> bool {
    match words {
        [first] => first == "GRANT",
        [first, second, ..] => match first.as_str() {
            "CREATE" | "ALTER" => matches!(second.as_str(), "USER" | "ROLE" | "LOGIN"),
            "SET" => second == "PASSWORD",
            "GRANT" => true,
            _ => false,
        },
        [] => false,
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
                ..DropOptions::default()
            },
        };
        assert_eq!(
            statements(Dialect::SqlServer, &drop, false).unwrap(),
            ["DROP USER app", "DROP LOGIN app"]
        );
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

    #[test]
    fn a_masked_preview_never_holds_the_password() {
        let mut s = spec("app");
        s.password = Some("S3cret!".to_owned());
        s.connection_limit = Some(2);
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

//! Who can do what on a server: its users and roles ([`Connection::principals`]) and the
//! privileges of one of them ([`Connection::principal_privileges`]), and the changes the
//! app makes to either ([`SecurityChange`], turned into SQL by `platypus_sql::security`).
//!
//! [`Connection::principals`]: crate::Connection::principals
//! [`Connection::principal_privileges`]: crate::Connection::principal_privileges

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{Connection, Result, TextRowExt, query_text};

/// Whether a [`Principal`] logs in or only groups privileges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalKind {
    #[default]
    User,
    Role,
}

impl PrincipalKind {
    /// A role when `is_role`, a user otherwise.
    #[must_use]
    pub fn of(is_role: bool) -> Self {
        if is_role { Self::Role } else { Self::User }
    }
}

/// The memberships `sql` lists, one a row (member, role): the roles of each member.
///
/// # Errors
/// When the query fails.
pub async fn memberships(conn: &dyn Connection, sql: &str) -> Result<HashMap<String, Vec<String>>> {
    let mut member_of: HashMap<String, Vec<String>> = HashMap::new();
    for r in query_text(conn, sql).await? {
        member_of.entry(r.text(0)).or_default().push(r.text(1));
    }
    Ok(member_of)
}

/// A yes/no attribute of a user or a role. Each database has some of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalFlag {
    /// May log in (PostgreSQL `LOGIN`).
    Login,
    Superuser,
    /// May create databases (PostgreSQL `CREATEDB`).
    CreateDb,
    /// May create roles (PostgreSQL `CREATEROLE`).
    CreateRole,
    /// Uses the privileges of the roles it belongs to without `SET ROLE` (PostgreSQL).
    Inherit,
    Replication,
    /// Skips row level security (PostgreSQL `BYPASSRLS`).
    BypassRls,
    /// The account cannot log in (MySQL `ACCOUNT LOCK`).
    Locked,
    /// The password must be changed at the next login (MySQL `PASSWORD EXPIRE`).
    PasswordExpired,
}

/// A user or a role of the server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Principal {
    pub name: String,
    /// The host part of a MySQL or MariaDB account (`user@host`).
    pub host: Option<String>,
    pub kind: PrincipalKind,
    /// The attributes that are on.
    pub flags: Vec<PrincipalFlag>,
    /// How many sessions it may have at once; `None` for no limit.
    pub connection_limit: Option<i64>,
    /// When its password expires, as the server writes the time.
    pub valid_until: Option<String>,
    /// How it logs in: the authentication plugin (MySQL), the login kind (SQL Server).
    pub authentication: Option<String>,
    /// The name of the server login a SQL Server user is mapped to: it can differ from the
    /// user's own. `None` when the user has no login.
    pub login: Option<String>,
    /// The roles it belongs to.
    pub member_of: Vec<String>,
}

impl Principal {
    /// Whether attribute `flag` is on.
    #[must_use]
    pub fn has(&self, flag: PrincipalFlag) -> bool {
        self.flags.contains(&flag)
    }
}

/// What a [`PrivilegeGrant`] is on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantKind {
    /// The whole server (MySQL `*.*`, ClickHouse `*.*`).
    #[default]
    Server,
    Database,
    Schema,
    Table,
    Column,
    Routine,
}

/// A privilege a principal has been given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PrivilegeGrant {
    pub kind: GrantKind,
    /// The schema (or MySQL database, ClickHouse database) the object is in.
    pub schema: Option<String>,
    /// The table, column's table or routine; `None` for what is on a whole schema or
    /// database.
    pub object: Option<String>,
    /// The column, for a [`GrantKind::Column`] grant.
    pub column: Option<String>,
    /// `SELECT`, `INSERT`, `USAGE`, …, as the server writes it.
    pub privilege: String,
    /// The principal may pass it on (`WITH GRANT OPTION`).
    pub grantable: bool,
}

/// A user or role named in a statement: `name`, and the host of a MySQL account.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grantee {
    pub name: String,
    #[serde(default)]
    pub host: Option<String>,
}

/// What a grant or a revoke is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrantTarget {
    /// Everything the server has (MySQL, ClickHouse `*.*`).
    Server,
    /// A database: PostgreSQL's `DATABASE`, MySQL's and ClickHouse's database; in SQL
    /// Server the database the session is in.
    Database { name: String },
    /// A schema itself (PostgreSQL's `USAGE`, `CREATE`).
    Schema { name: String },
    /// One table or view.
    Table { schema: String, name: String },
    /// Every table and view of a schema, including those created later where the database
    /// can say so.
    AllTables { schema: String },
    /// Every function and procedure of a schema.
    AllRoutines { schema: String },
}

impl GrantTarget {
    /// The kind of object it is on, without its name.
    #[must_use]
    pub fn class(&self) -> TargetClass {
        match self {
            Self::Server => TargetClass::Server,
            Self::Database { .. } => TargetClass::Database,
            Self::Schema { .. } => TargetClass::Schema,
            Self::Table { .. } => TargetClass::Table,
            Self::AllTables { .. } => TargetClass::AllTables,
            Self::AllRoutines { .. } => TargetClass::AllRoutines,
        }
    }
}

/// The kind of object a [`GrantTarget`] is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetClass {
    Server,
    Database,
    Schema,
    Table,
    AllTables,
    AllRoutines,
}

/// Grant or revoke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantAction {
    Grant,
    Revoke,
}

/// A user or role to create or alter, as the form fills it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalSpec {
    pub name: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub kind: PrincipalKind,
    /// Attributes to turn on.
    #[serde(default)]
    pub flags_on: Vec<PrincipalFlag>,
    /// Attributes to turn off. Altering only; a created principal starts with the
    /// database's defaults.
    #[serde(default)]
    pub flags_off: Vec<PrincipalFlag>,
    /// Sessions allowed at once; `-1` removes the limit. `None` leaves it.
    #[serde(default)]
    pub connection_limit: Option<i64>,
    /// When the password expires; empty removes it. `None` leaves it.
    #[serde(default)]
    pub valid_until: Option<String>,
    /// The password to set; `None` or empty leaves it. It is only ever in the statement
    /// that runs: previews and the query log show it masked.
    #[serde(default)]
    pub password: Option<String>,
    /// The new name, when renaming. Altering only.
    #[serde(default)]
    pub rename_to: Option<String>,
    /// Altering, SQL Server: the name of the login the user is mapped to
    /// ([`Principal::login`]). Login-level changes (password, lock) need it.
    #[serde(default)]
    pub login: Option<String>,
}

/// What dropping a principal also does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropOptions {
    /// Gives what it owns to this role first (PostgreSQL `REASSIGN OWNED`).
    #[serde(default)]
    pub reassign_to: Option<String>,
    /// Drops what it owns and the privileges it was given (PostgreSQL `DROP OWNED`).
    #[serde(default)]
    pub drop_owned: bool,
    /// Drops the server login the user was created with (SQL Server).
    #[serde(default)]
    pub drop_login: bool,
    /// The name of the login to drop with `drop_login` ([`Principal::login`]); without it
    /// there is nothing to drop.
    #[serde(default)]
    pub login: Option<String>,
}

/// A change to the users, roles and privileges of a server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum SecurityChange {
    Create {
        spec: PrincipalSpec,
    },
    Alter {
        spec: PrincipalSpec,
    },
    Drop {
        name: String,
        #[serde(default)]
        host: Option<String>,
        #[serde(default)]
        kind: PrincipalKind,
        #[serde(default)]
        options: DropOptions,
    },
    /// Grants or revokes privileges on an object.
    Privileges {
        action: GrantAction,
        grantees: Vec<Grantee>,
        target: GrantTarget,
        /// Names from `platypus_sql::security::privileges`; `ALL` for all of them.
        privileges: Vec<String>,
        /// Grant: `WITH GRANT OPTION`. Revoke: only takes the grant option away.
        #[serde(default)]
        grant_option: bool,
        /// Revoke: also what others were given through the grantee.
        #[serde(default)]
        cascade: bool,
    },
    /// Adds `members` to `role`, or takes them out.
    Membership {
        action: GrantAction,
        role: String,
        members: Vec<Grantee>,
        /// Grant: the member may add others (`WITH ADMIN OPTION`).
        #[serde(default)]
        admin_option: bool,
    },
}

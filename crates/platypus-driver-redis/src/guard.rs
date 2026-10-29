//! The checks a batch passes before its `MULTI`/`EXEC`. The commands that create a key, a
//! field or a member only when it is absent (`RENAMENX`, `SET … NX`, `JSON.SET … NX`,
//! `HSETNX`, `ZADD … NX`, `SADD`) answer 0 or nil inside `EXEC` when it exists: nothing is
//! saved and nothing says so. The batch is refused before it runs instead, as a duplicate
//! key is in SQL. What an earlier command of the batch deletes is free for a later one.

use redis::{Cmd, Value};

use crate::command::{display, upper};

/// Something a command creates: a key, or an item of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Key(Vec<u8>),
    Field(Vec<u8>, Vec<u8>),
    Zset(Vec<u8>, Vec<u8>),
    Member(Vec<u8>, Vec<u8>),
}

impl Target {
    fn key(&self) -> &[u8] {
        match self {
            Self::Key(k) | Self::Field(k, _) | Self::Zset(k, _) | Self::Member(k, _) => k,
        }
    }

    /// Whether deleting `self` frees `other`: a key frees all its items.
    fn covers(&self, other: &Self) -> bool {
        self == other || matches!(self, Self::Key(k) if k == other.key())
    }

    /// The command whose reply says whether the target exists.
    #[must_use]
    pub fn check(&self) -> Cmd {
        let (name, key, item) = match self {
            Self::Key(k) => ("EXISTS", k, None),
            Self::Field(k, f) => ("HEXISTS", k, Some(f)),
            Self::Zset(k, m) => ("ZSCORE", k, Some(m)),
            Self::Member(k, m) => ("SISMEMBER", k, Some(m)),
        };
        let mut cmd = redis::cmd(name);
        cmd.arg(key.as_slice());
        if let Some(item) = item {
            cmd.arg(item.as_slice());
        }
        cmd
    }

    /// Whether `reply`, to [`Self::check`], says the target exists.
    #[must_use]
    pub fn exists(reply: &Value) -> bool {
        !matches!(reply, Value::Int(0) | Value::Nil | Value::Boolean(false))
    }

    /// The error of a batch that would create `self` where it exists.
    #[must_use]
    pub fn conflict(&self) -> String {
        let key = display(self.key());
        let what = match self {
            Self::Key(_) => format!("the key `{key}`"),
            Self::Field(_, f) => format!("the field `{}` of `{key}`", display(f)),
            Self::Zset(_, m) | Self::Member(_, m) => {
                format!("the member `{}` of `{key}`", display(m))
            }
        };
        format!("{what} already exists: nothing was saved")
    }
}

/// What `args` creates, and what it deletes.
fn effects(args: &[Vec<u8>]) -> (Option<Target>, Vec<Target>) {
    let name = upper(args.first().map_or(&[][..], Vec::as_slice));
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let nx = |from: usize| args.iter().skip(from).any(|a| upper(a) == "NX");
    let items = |make: fn(Vec<u8>, Vec<u8>) -> Target| {
        args.iter()
            .skip(2)
            .map(|m| make(arg(1), m.clone()))
            .collect()
    };
    match name.as_str() {
        "DEL" | "UNLINK" => (None, args[1..].iter().cloned().map(Target::Key).collect()),
        "RENAME" => (None, vec![Target::Key(arg(1))]),
        "RENAMENX" => (Some(Target::Key(arg(2))), vec![Target::Key(arg(1))]),
        "SET" if nx(3) => (Some(Target::Key(arg(1))), Vec::new()),
        "JSON.SET" if nx(4) => (Some(Target::Key(arg(1))), Vec::new()),
        "HSETNX" => (Some(Target::Field(arg(1), arg(2))), Vec::new()),
        "HDEL" => (None, items(Target::Field)),
        "ZADD" if args.len() == 5 && upper(&args[2]) == "NX" => {
            (Some(Target::Zset(arg(1), arg(4))), Vec::new())
        }
        "ZREM" => (None, items(Target::Zset)),
        "SADD" if args.len() == 3 => (Some(Target::Member(arg(1), arg(2))), Vec::new()),
        "SREM" => (None, items(Target::Member)),
        _ => (None, Vec::new()),
    }
}

/// The targets of `commands` to check on the server, by command index; or the index of a
/// command that creates what an earlier one of the batch created.
pub fn checks(commands: &[Vec<Vec<u8>>]) -> Result<Vec<(usize, Target)>, (usize, Target)> {
    let mut freed: Vec<Target> = Vec::new();
    let mut created: Vec<Target> = Vec::new();
    let mut out = Vec::new();
    for (i, args) in commands.iter().enumerate() {
        let (made, deletes) = effects(args);
        for target in deletes {
            created.retain(|c| !target.covers(c));
            freed.push(target);
        }
        if let Some(target) = made {
            if created.contains(&target) {
                return Err((i, target));
            }
            if !freed.iter().any(|f| f.covers(&target)) {
                out.push((i, target.clone()));
            }
            created.push(target);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::tokenize;

    fn batch(lines: &[&str]) -> Vec<Vec<Vec<u8>>> {
        lines.iter().map(|l| tokenize(l).unwrap()).collect()
    }

    fn key(k: &str) -> Target {
        Target::Key(k.as_bytes().to_vec())
    }

    #[test]
    fn checks_what_the_batch_creates() {
        let checks = checks(&batch(&[
            "RENAMENX a b",
            "SET c v NX",
            "SET d v KEEPTTL",
            "JSON.SET j $ 1 NX",
            "ZADD z NX 1 m",
            "ZADD z XX 2 m",
        ]))
        .unwrap();
        let targets: Vec<_> = checks.iter().map(|(i, t)| (*i, t.clone())).collect();
        assert_eq!(
            targets,
            [
                (0, key("b")),
                (1, key("c")),
                (3, key("j")),
                (4, Target::Zset(b"z".to_vec(), b"m".to_vec())),
            ]
        );
    }

    #[test]
    fn a_deleted_target_is_free_and_a_second_creation_conflicts() {
        let freed = checks(&batch(&[
            "HDEL h g",
            "HDEL h f",
            "HSETNX h g v",
            "DEL k",
            "SET k 1 NX",
        ]));
        assert_eq!(freed.unwrap(), []);
        let twice = checks(&batch(&["SADD s m", "SADD s m"]));
        assert_eq!(
            twice.unwrap_err(),
            (1, Target::Member(b"s".to_vec(), b"m".to_vec()))
        );
        // A key renamed away frees its name for a new one.
        assert_eq!(
            checks(&batch(&["RENAMENX a b", "SET a 1 NX"]))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn reads_the_check_replies() {
        assert!(Target::exists(&Value::Int(1)));
        assert!(Target::exists(&Value::BulkString(b"1.5".to_vec())));
        assert!(!Target::exists(&Value::Int(0)));
        assert!(!Target::exists(&Value::Nil));
        assert_eq!(
            key("b").conflict(),
            "the key `b` already exists: nothing was saved"
        );
    }
}

//! Git for a folder of scripts, through the system `git` executable. Arguments are always
//! passed as a list, never through a shell, and Git is told never to ask anything on a
//! terminal, so a missing credential is an error instead of a hang.

mod parse;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

use serde::{Deserialize, Serialize};

pub use parse::{Branch, DiffLine, FileDiff, FileStatus, Hunk, LineKind, parse_diff, parse_status};

/// Time given to a command that stays on this computer.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);
/// Time given to a command that talks to a remote.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a Git operation failed.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// No `git` executable was found.
    #[error("Git is not installed")]
    NotInstalled,
    /// The folder is not a Git repository.
    #[error("this folder is not a Git repository")]
    NotRepo,
    /// Git does not know who the author is: the caller asks for a name and an email.
    #[error("Git does not know your name and email")]
    IdentityMissing,
    /// The remote wants credentials that Git cannot get without asking.
    #[error("authentication needed: configure a credential helper")]
    AuthNeeded,
    /// The branch has nowhere to push or pull from.
    #[error("the repository has no remote")]
    NoRemote,
    /// The branch does not track a remote branch.
    #[error("the branch has no upstream branch")]
    NoUpstream,
    /// A command ran past its time.
    #[error("Git did not answer in time")]
    Timeout,
    /// A path that does not name a file of the repository.
    #[error("not a file of the repository: {0}")]
    InvalidPath(String),
    /// Nothing to commit, or no message.
    #[error("{0}")]
    Invalid(String),
    /// Git failed; its own message.
    #[error("{0}")]
    Failed(String),
    /// Starting Git, or reading a file, failed.
    #[error("{0}")]
    Io(String),
}

impl GitError {
    /// A stable name for the UI to branch on.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotInstalled => "git_not_installed",
            Self::NotRepo => "git_not_repo",
            Self::IdentityMissing => "git_identity_missing",
            Self::AuthNeeded => "git_auth_needed",
            Self::NoRemote => "git_no_remote",
            Self::NoUpstream => "git_no_upstream",
            Self::Timeout => "git_timeout",
            Self::InvalidPath(_) | Self::Invalid(_) => "invalid",
            Self::Failed(_) => "git_failed",
            Self::Io(_) => "io",
        }
    }
}

type Result<T> = std::result::Result<T, GitError>;

/// The author of commits: set in the repository when Git has none.
#[derive(Debug, Clone, Deserialize)]
pub struct Identity {
    pub name: String,
    pub email: String,
}

/// The state of the folder as the Scripts panel shows it.
#[derive(Debug, Clone, Serialize)]
pub struct RepoStatus {
    /// False when the folder is not (the top of) a repository: the panel offers to start one.
    pub is_repo: bool,
    pub branch: Branch,
    pub files: Vec<FileStatus>,
    /// A merge stopped on conflicts and waits for them to be resolved or aborted.
    pub merging: bool,
}

/// What a pull did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PullOutcome {
    UpToDate,
    FastForwarded,
    Merged,
    /// Both sides have new commits: a fast-forward is not possible.
    Diverged,
    /// The merge stopped on these files.
    Conflicts {
        files: Vec<String>,
    },
}

/// The places Git is usually installed on Windows, when it is not on `PATH`.
fn windows_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            out.push(Path::new(&dir).join("Git").join("cmd").join("git.exe"));
        }
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        out.push(
            Path::new(&dir)
                .join("Programs")
                .join("Git")
                .join("cmd")
                .join("git.exe"),
        );
    }
    out
}

fn works(git: &Path) -> bool {
    let mut cmd = Command::new(git);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_window(&mut cmd);
    cmd.status().is_ok_and(|s| s.success())
}

/// The `git` executable: the one on `PATH`, else a usual install path on Windows.
#[must_use]
pub fn find_git() -> Option<PathBuf> {
    let on_path = PathBuf::from("git");
    if works(&on_path) {
        return Some(on_path);
    }
    if cfg!(windows) {
        return windows_candidates()
            .into_iter()
            .find(|p| p.is_file() && works(p));
    }
    None
}

/// Keeps a console window from flashing on Windows.
fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// What a finished command wrote.
struct Output {
    stdout: String,
    stderr: String,
    ok: bool,
}

fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    })
}

/// Runs `git args` in `dir`, killing it after `timeout`. A failing exit status is not an
/// error here: callers read `ok`.
fn run_raw(dir: &Path, args: &[&str], timeout: Duration) -> Result<Output> {
    let git = find_git().ok_or(GitError::NotInstalled)?;
    let mut cmd = Command::new(git);
    cmd.arg("--literal-pathspecs")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Never ask for a credential on a terminal, and keep the messages in English so
        // they can be recognised.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    hide_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| GitError::Io(e.to_string()))?;
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(|e| GitError::Io(e.to_string()))? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GitError::Timeout);
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    };
    let join =
        |h: Option<thread::JoinHandle<String>>| h.and_then(|h| h.join().ok()).unwrap_or_default();
    Ok(Output {
        stdout: join(out),
        stderr: join(err),
        ok: status.success(),
    })
}

/// Whether the message of a failed command means that credentials are missing.
fn needs_auth(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    [
        "terminal prompts disabled",
        "could not read username",
        "could not read password",
        "authentication failed",
        "permission denied (publickey",
        "host key verification failed",
    ]
    .iter()
    .any(|m| s.contains(m))
}

fn run(dir: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let out = run_raw(dir, args, timeout)?;
    if out.ok {
        return Ok(out.stdout);
    }
    if needs_auth(&out.stderr) {
        return Err(GitError::AuthNeeded);
    }
    let msg = out.stderr.trim();
    Err(GitError::Failed(if msg.is_empty() {
        out.stdout.trim().to_owned()
    } else {
        msg.to_owned()
    }))
}

fn local(dir: &Path, args: &[&str]) -> Result<String> {
    run(dir, args, LOCAL_TIMEOUT)
}

fn network(dir: &Path, args: &[&str]) -> Result<String> {
    run(dir, args, NETWORK_TIMEOUT)
}

/// Whether `dir` is the top folder of a repository (not a folder inside another one, whose
/// history would mix in other files).
fn is_repo_top(dir: &Path) -> Result<bool> {
    let out = run_raw(
        dir,
        &["rev-parse", "--is-inside-work-tree", "--show-prefix"],
        LOCAL_TIMEOUT,
    )?;
    let mut lines = out.stdout.lines();
    Ok(out.ok && lines.next() == Some("true") && lines.next().is_none_or(str::is_empty))
}

fn require_repo(dir: &Path) -> Result<()> {
    if is_repo_top(dir)? {
        Ok(())
    } else {
        Err(GitError::NotRepo)
    }
}

fn merging(dir: &Path) -> Result<bool> {
    Ok(run_raw(
        dir,
        &["rev-parse", "-q", "--verify", "MERGE_HEAD"],
        LOCAL_TIMEOUT,
    )?
    .ok)
}

/// Turns the folder into a Git repository; does nothing if it already is one.
pub fn init(dir: &Path) -> Result<()> {
    if is_repo_top(dir)? {
        return Ok(());
    }
    local(dir, &["init", "--quiet"]).map(drop)
}

/// The state of the folder: files changed, the branch and how far it is from its upstream.
pub fn status(dir: &Path) -> Result<RepoStatus> {
    if !is_repo_top(dir)? {
        return Ok(RepoStatus {
            is_repo: false,
            branch: Branch::default(),
            files: Vec::new(),
            merging: false,
        });
    }
    let out = local(
        dir,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
    )?;
    let parsed = parse_status(&out);
    Ok(RepoStatus {
        is_repo: true,
        branch: parsed.branch,
        files: parsed.files,
        merging: merging(dir)?,
    })
}

/// A path as the panel sends it: relative, with `/`, inside the folder.
fn check_path(path: &str) -> Result<()> {
    let bad = path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', '\0'])
        || path
            .split('/')
            .any(|c| c.is_empty() || c == ".." || c == ".git");
    if bad {
        Err(GitError::InvalidPath(path.to_owned()))
    } else {
        Ok(())
    }
}

/// The changes of a file against the last commit: what the working copy has now.
pub fn diff(dir: &Path, path: &str) -> Result<FileDiff> {
    check_path(path)?;
    require_repo(dir)?;
    let has_head = run_raw(dir, &["rev-parse", "-q", "--verify", "HEAD"], LOCAL_TIMEOUT)?.ok;
    let tracked = !local(dir, &["ls-files", "--", path])?.trim().is_empty();
    if has_head && tracked {
        let out = local(
            dir,
            &["diff", "HEAD", "--no-color", "--no-ext-diff", "--", path],
        )?;
        return Ok(parse_diff(&out));
    }
    // Git has no commit to compare with: the whole file is new.
    match fs::read(dir.join(path)) {
        Ok(bytes) if bytes.contains(&0) => Ok(FileDiff {
            binary: true,
            hunks: Vec::new(),
        }),
        Ok(bytes) => Ok(parse::added_file_diff(&String::from_utf8_lossy(&bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileDiff::default()),
        Err(e) => Err(GitError::Io(format!("{path}: {e}"))),
    }
}

/// The author Git would use, if it knows both a name and an email.
fn has_identity(dir: &Path) -> Result<bool> {
    for key in ["user.name", "user.email"] {
        let out = run_raw(dir, &["config", "--get", key], LOCAL_TIMEOUT)?;
        if !out.ok || out.stdout.trim().is_empty() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Makes sure commits have an author: sets `identity` in this repository when Git has none,
/// and asks for one (`IdentityMissing`) when it has none and none is given.
fn ensure_identity(dir: &Path, identity: Option<&Identity>) -> Result<()> {
    if has_identity(dir)? {
        return Ok(());
    }
    let id = identity
        .filter(|i| !i.name.trim().is_empty() && !i.email.trim().is_empty())
        .ok_or(GitError::IdentityMissing)?;
    local(dir, &["config", "user.name", id.name.trim()])?;
    local(dir, &["config", "user.email", id.email.trim()])?;
    Ok(())
}

/// Stages `paths` and commits just those with `message`; returns the short id of the commit.
/// While a merge waits to be concluded the whole index is committed, as Git requires.
pub fn commit(
    dir: &Path,
    paths: &[String],
    message: &str,
    identity: Option<&Identity>,
) -> Result<String> {
    require_repo(dir)?;
    if message.trim().is_empty() {
        return Err(GitError::Invalid("the commit message is empty".into()));
    }
    if paths.is_empty() {
        return Err(GitError::Invalid("no file selected".into()));
    }
    paths.iter().try_for_each(|p| check_path(p))?;
    ensure_identity(dir, identity)?;
    let mut add = vec!["add", "-A", "--"];
    add.extend(paths.iter().map(String::as_str));
    local(dir, &add)?;
    let has_head = run_raw(dir, &["rev-parse", "-q", "--verify", "HEAD"], LOCAL_TIMEOUT)?.ok;
    let mut args = vec!["commit", "--quiet", "-m", message.trim()];
    if has_head && !merging(dir)? {
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
    }
    local(dir, &args)?;
    Ok(local(dir, &["rev-parse", "--short", "HEAD"])?
        .trim()
        .to_owned())
}

/// Brings in what the remote has, without changing any file.
pub fn fetch(dir: &Path) -> Result<()> {
    require_repo(dir)?;
    network(dir, &["fetch", "--quiet"]).map(drop)
}

/// The commits only here and only on the upstream.
fn ahead_behind(dir: &Path) -> Result<(u32, u32)> {
    let b = status(dir)?.branch;
    if b.upstream.is_none() {
        return Err(GitError::NoUpstream);
    }
    Ok((b.ahead, b.behind))
}

/// Fetches, then fast-forwards. When both sides have new commits nothing is changed and the
/// result is `Diverged`: the caller may then offer [`pull_merge`].
pub fn pull(dir: &Path) -> Result<PullOutcome> {
    require_repo(dir)?;
    if run_raw(dir, &["remote"], LOCAL_TIMEOUT)?
        .stdout
        .trim()
        .is_empty()
    {
        return Err(GitError::NoRemote);
    }
    ahead_behind(dir)?;
    network(dir, &["fetch", "--quiet"])?;
    let (ahead, behind) = ahead_behind(dir)?;
    if behind == 0 {
        return Ok(PullOutcome::UpToDate);
    }
    if ahead > 0 {
        return Ok(PullOutcome::Diverged);
    }
    local(dir, &["merge", "--ff-only", "--quiet", "@{upstream}"])?;
    Ok(PullOutcome::FastForwarded)
}

/// The files a stopped merge could not combine.
fn conflicted(dir: &Path) -> Result<Vec<String>> {
    let out = local(dir, &["diff", "--name-only", "--diff-filter=U", "-z"])?;
    Ok(out
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Pulls with a merge (`git pull --no-rebase`), for a divergence. A merge that stops on
/// conflicts is left in progress for the user to resolve and commit, or to [`merge_abort`].
pub fn pull_merge(dir: &Path, identity: Option<&Identity>) -> Result<PullOutcome> {
    require_repo(dir)?;
    ensure_identity(dir, identity)?;
    ahead_behind(dir)?;
    let out = run_raw(
        dir,
        &["pull", "--no-rebase", "--no-edit", "--quiet"],
        NETWORK_TIMEOUT,
    )?;
    if out.ok {
        return Ok(PullOutcome::Merged);
    }
    if needs_auth(&out.stderr) {
        return Err(GitError::AuthNeeded);
    }
    let files = conflicted(dir)?;
    if files.is_empty() {
        return Err(GitError::Failed(out.stderr.trim().to_owned()));
    }
    Ok(PullOutcome::Conflicts { files })
}

/// Gives up a merge that stopped on conflicts.
pub fn merge_abort(dir: &Path) -> Result<()> {
    require_repo(dir)?;
    local(dir, &["merge", "--abort"]).map(drop)
}

/// Pushes the branch; a branch with no upstream is published on the first remote and starts
/// tracking it.
pub fn push(dir: &Path) -> Result<()> {
    require_repo(dir)?;
    let remotes = local(dir, &["remote"])?;
    let Some(remote) = remotes.lines().next() else {
        return Err(GitError::NoRemote);
    };
    if status(dir)?.branch.upstream.is_some() {
        network(dir, &["push", "--quiet"])
    } else {
        network(dir, &["push", "--quiet", "-u", remote, "HEAD"])
    }
    .map(drop)
}

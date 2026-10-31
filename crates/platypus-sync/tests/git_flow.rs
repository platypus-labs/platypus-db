//! The whole Git flow against the real `git` executable: skipped when Git is missing.

use std::fs;
use std::path::Path;
use std::process::Command;

use platypus_sync::git::{self, GitError, Identity, LineKind, PullOutcome};

fn id() -> Identity {
    Identity {
        name: "Test".into(),
        email: "test@example.com".into(),
    }
}

fn git_ok(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn badges(dir: &Path) -> Vec<(String, char)> {
    let mut all: Vec<_> = git::status(dir)
        .unwrap()
        .files
        .into_iter()
        .map(|f| (f.path, f.badge))
        .collect();
    all.sort();
    all
}

fn one(p: &str) -> Vec<String> {
    vec![p.to_owned()]
}

#[test]
fn local_flow() {
    if git::find_git().is_none() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("scripts");
    fs::create_dir(&dir).unwrap();

    assert!(!git::status(&dir).unwrap().is_repo);
    assert!(matches!(git::diff(&dir, "a.sql"), Err(GitError::NotRepo)));
    git::init(&dir).unwrap();
    git::init(&dir).unwrap();
    assert!(git::status(&dir).unwrap().is_repo);

    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub/a.sql"), "select 1;\n").unwrap();
    assert_eq!(badges(&dir), [("sub/a.sql".to_owned(), '?')]);
    let d = git::diff(&dir, "sub/a.sql").unwrap();
    assert_eq!(d.hunks[0].lines[0].kind, LineKind::Added);

    // Rejected input never reaches Git.
    assert!(matches!(
        git::diff(&dir, "../x"),
        Err(GitError::InvalidPath(_))
    ));
    assert!(matches!(
        git::commit(&dir, &one("sub/a.sql"), "  ", None),
        Err(GitError::Invalid(_))
    ));
    assert!(matches!(
        git::commit(&dir, &[], "m", None),
        Err(GitError::Invalid(_))
    ));

    // Make sure Git has no author of its own, so the identity is asked once and then kept.
    git_ok(&dir, &["config", "user.name", ""]);
    git_ok(&dir, &["config", "user.email", ""]);
    assert!(matches!(
        git::commit(&dir, &one("sub/a.sql"), "first", None),
        Err(GitError::IdentityMissing)
    ));
    git::commit(&dir, &one("sub/a.sql"), "first", Some(&id())).unwrap();
    assert_eq!(badges(&dir).len(), 0);
    assert!(git::status(&dir).unwrap().branch.has_commits);

    fs::write(dir.join("sub/a.sql"), "select 1;\nselect 2;\n").unwrap();
    fs::write(dir.join("b.sql"), "select 3;\n").unwrap();
    assert_eq!(
        badges(&dir),
        [("b.sql".to_owned(), '?'), ("sub/a.sql".to_owned(), 'M')]
    );
    let d = git::diff(&dir, "sub/a.sql").unwrap();
    assert_eq!(d.hunks.len(), 1);
    let added: Vec<_> = d.hunks[0]
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Added)
        .collect();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].text, "select 2;");

    // Only the selected file is committed; a deletion is committed too.
    git::commit(&dir, &one("sub/a.sql"), "second", None).unwrap();
    assert_eq!(badges(&dir), [("b.sql".to_owned(), '?')]);
    fs::remove_file(dir.join("sub/a.sql")).unwrap();
    assert_eq!(badges(&dir)[1], ("sub/a.sql".to_owned(), 'D'));
    assert!(
        git::diff(&dir, "sub/a.sql").unwrap().hunks[0]
            .lines
            .iter()
            .all(|l| l.kind == LineKind::Removed)
    );
    git::commit(&dir, &one("sub/a.sql"), "delete", None).unwrap();

    // No remote: pull and push say so.
    assert!(matches!(git::pull(&dir), Err(GitError::NoRemote)));
    assert!(matches!(git::push(&dir), Err(GitError::NoRemote)));
}

#[test]
fn remote_flow() {
    if git::find_git().is_none() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let bare = root.join("remote.git");
    fs::create_dir(&bare).unwrap();
    git_ok(
        &bare,
        &["init", "--bare", "--quiet", "--initial-branch=main"],
    );
    let url = bare.to_str().unwrap();

    let one_dir = root.join("one");
    fs::create_dir(&one_dir).unwrap();
    git::init(&one_dir).unwrap();
    git_ok(&one_dir, &["checkout", "-q", "-b", "main"]);
    // Line endings as written, whatever the machine's `core.autocrlf` (true on Windows).
    git_ok(&one_dir, &["config", "core.autocrlf", "false"]);
    git_ok(&one_dir, &["remote", "add", "origin", url]);
    fs::write(one_dir.join("a.sql"), "select 1;\n").unwrap();
    git::commit(&one_dir, &one("a.sql"), "first", Some(&id())).unwrap();
    // No upstream yet: pull asks for one, push publishes the branch.
    assert!(matches!(git::pull(&one_dir), Err(GitError::NoUpstream)));
    git::push(&one_dir).unwrap();
    let b = git::status(&one_dir).unwrap().branch;
    assert_eq!(b.upstream.as_deref(), Some("origin/main"));
    assert_eq!((b.ahead, b.behind), (0, 0));

    // A second clone changes the file and pushes.
    let two_dir = root.join("two");
    git_ok(
        root,
        &["clone", "-q", "-c", "core.autocrlf=false", url, "two"],
    );
    git_ok(&two_dir, &["config", "user.name", "Two"]);
    git_ok(&two_dir, &["config", "user.email", "two@example.com"]);
    fs::write(two_dir.join("a.sql"), "select 1;\nselect 2;\n").unwrap();
    git::commit(&two_dir, &one("a.sql"), "two", None).unwrap();
    git::push(&two_dir).unwrap();

    // The first one sees nothing until it fetches, then fast-forwards.
    assert_eq!(git::status(&one_dir).unwrap().branch.behind, 0);
    git::fetch(&one_dir).unwrap();
    assert_eq!(git::status(&one_dir).unwrap().branch.behind, 1);
    assert_eq!(git::pull(&one_dir).unwrap(), PullOutcome::FastForwarded);
    assert_eq!(
        fs::read_to_string(one_dir.join("a.sql")).unwrap(),
        "select 1;\nselect 2;\n"
    );
    assert_eq!(git::pull(&one_dir).unwrap(), PullOutcome::UpToDate);

    // Both sides change: the pull is refused as divergent, and nothing is touched.
    fs::write(one_dir.join("b.sql"), "select 10;\n").unwrap();
    git::commit(&one_dir, &one("b.sql"), "one b", None).unwrap();
    fs::write(two_dir.join("c.sql"), "select 20;\n").unwrap();
    git::commit(&two_dir, &one("c.sql"), "two c", None).unwrap();
    git::push(&two_dir).unwrap();
    assert_eq!(git::pull(&one_dir).unwrap(), PullOutcome::Diverged);
    let b = git::status(&one_dir).unwrap().branch;
    assert_eq!((b.ahead, b.behind), (1, 1));
    assert!(!one_dir.join("c.sql").exists());
    assert_eq!(
        git::pull_merge(&one_dir, None).unwrap(),
        PullOutcome::Merged
    );
    assert!(one_dir.join("c.sql").exists());
    git::push(&one_dir).unwrap();

    git_ok(&two_dir, &["pull", "--ff-only", "--quiet"]);

    // Both edit the same line: the merge stops and names the file; abort restores.
    fs::write(two_dir.join("a.sql"), "two\n").unwrap();
    git::commit(&two_dir, &one("a.sql"), "two a", None).unwrap();
    fs::write(one_dir.join("a.sql"), "one\n").unwrap();
    git::commit(&one_dir, &one("a.sql"), "one a", None).unwrap();
    git::push(&two_dir).unwrap();
    git::fetch(&one_dir).unwrap();
    assert_eq!(git::pull(&one_dir).unwrap(), PullOutcome::Diverged);
    assert_eq!(
        git::pull_merge(&one_dir, None).unwrap(),
        PullOutcome::Conflicts {
            files: vec!["a.sql".into()]
        }
    );
    let s = git::status(&one_dir).unwrap();
    assert!(s.merging);
    assert_eq!(s.files[0].badge, 'U');
    git::merge_abort(&one_dir).unwrap();
    let s = git::status(&one_dir).unwrap();
    assert!(!s.merging && s.files.is_empty());
    assert_eq!(fs::read_to_string(one_dir.join("a.sql")).unwrap(), "one\n");

    // Resolving the conflict and committing concludes a merge.
    assert!(matches!(
        git::pull_merge(&one_dir, None).unwrap(),
        PullOutcome::Conflicts { .. }
    ));
    fs::write(one_dir.join("a.sql"), "both\n").unwrap();
    git::commit(&one_dir, &one("a.sql"), "resolved", None).unwrap();
    let s = git::status(&one_dir).unwrap();
    assert!(!s.merging && s.files.is_empty());
    assert_eq!(s.branch.ahead, 2);
}

#[test]
fn a_shared_projects_scripts_use_the_repository_around_them() {
    if git::find_git().is_none() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    // A team's repository: the shared project sits in a folder of it, beside other files.
    let repo = tmp.path().join("team-repo");
    let project = repo.join("db").join("sales");
    let scripts = project.join("scripts");
    fs::create_dir_all(&scripts).unwrap();
    git_ok(&repo, &["init", "--quiet", "--initial-branch=main"]);
    git_ok(&repo, &["config", "user.name", "Team"]);
    git_ok(&repo, &["config", "user.email", "team@example.com"]);
    fs::write(project.join("platypus-project.toml"), "version = 1\n").unwrap();
    fs::write(repo.join("README.md"), "team\n").unwrap();
    fs::write(scripts.join("a.sql"), "select 1;\n").unwrap();
    git_ok(&repo, &["add", "-A"]);
    git_ok(&repo, &["commit", "-q", "-m", "start"]);

    // The scripts folder is a repository for the panel, and init leaves it alone.
    assert!(git::status(&scripts).unwrap().is_repo);
    git::init(&scripts).unwrap();
    assert!(
        !scripts.join(".git").exists(),
        "no repository inside the team's"
    );

    // Only the scripts folder's files are listed, by their path in it.
    fs::write(scripts.join("a.sql"), "select 2;\n").unwrap();
    fs::create_dir(scripts.join("sub")).unwrap();
    fs::write(scripts.join("sub/b.sql"), "select 3;\n").unwrap();
    fs::write(repo.join("README.md"), "changed\n").unwrap();
    fs::write(project.join("notes.txt"), "x\n").unwrap();
    assert_eq!(
        badges(&scripts),
        [("a.sql".to_owned(), 'M'), ("sub/b.sql".to_owned(), '?')]
    );
    let d = git::diff(&scripts, "a.sql").unwrap();
    assert!(
        d.hunks[0]
            .lines
            .iter()
            .any(|l| l.kind == LineKind::Added && l.text == "select 2;")
    );
    assert_eq!(
        git::diff(&scripts, "sub/b.sql").unwrap().hunks[0].lines[0].kind,
        LineKind::Added
    );

    // A commit takes the scripts named, in the team's repository, and nothing else.
    git::commit(&scripts, &one("a.sql"), "edit a", None).unwrap();
    assert_eq!(badges(&scripts), [("sub/b.sql".to_owned(), '?')]);
    let committed = Command::new("git")
        .args(["show", "--name-only", "--format=", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&committed.stdout).trim(),
        "db/sales/scripts/a.sql"
    );
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let dirty = String::from_utf8_lossy(&dirty.stdout);
    assert!(
        dirty.contains("README.md") && dirty.contains("notes.txt"),
        "{dirty}"
    );
    assert!(matches!(git::push(&scripts), Err(GitError::NoRemote)));

    // A folder inside a repository that is not a shared project's scripts is not one.
    let other = repo.join("misc");
    fs::create_dir(&other).unwrap();
    assert!(!git::status(&other).unwrap().is_repo);
}

/// A repository planted with a hook and an fsmonitor command of its own (in a shared folder,
/// `.git` comes along): Git run by the app starts neither. A plain `git` does, which shows
/// the plant works here; where it does not (no shell for hooks), only the app side is checked.
#[test]
fn a_planted_repository_runs_nothing() {
    if git::find_git().is_none() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("scripts");
    fs::create_dir(&dir).unwrap();
    git_ok(&dir, &["init", "-q"]);
    git_ok(&dir, &["config", "user.name", "Test"]);
    git_ok(&dir, &["config", "user.email", "test@example.com"]);
    git_ok(
        &dir,
        &["config", "core.fsmonitor", "echo ran > .git/fsmonitor-ran"],
    );
    let hook = dir.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\necho ran > .git/hook-ran\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(dir.join("a.sql"), "select 1;\n").unwrap();
    let fsmonitor = dir.join(".git/fsmonitor-ran");
    let hook_ran = dir.join(".git/hook-ran");

    // The control: plain Git runs both.
    let _ = Command::new("git").arg("status").current_dir(&dir).output();
    git_ok(&dir, &["add", "a.sql"]);
    let _ = Command::new("git")
        .args(["commit", "-q", "-m", "control"])
        .current_dir(&dir)
        .output();
    let planted = (fsmonitor.exists(), hook_ran.exists());
    if planted != (true, true) {
        eprintln!("the plant did not run with plain git here: {planted:?}");
    }
    let _ = fs::remove_file(&fsmonitor);
    let _ = fs::remove_file(&hook_ran);

    fs::write(dir.join("b.sql"), "select 2;\n").unwrap();
    assert!(git::status(&dir).unwrap().is_repo);
    git::commit(&dir, &one("b.sql"), "add b", None).unwrap();
    assert!(!fsmonitor.exists(), "the fsmonitor command ran");
    assert!(!hook_ran.exists(), "the pre-commit hook ran");

    // Settings that run commands on their own are refused, whoever wrote them; Git LFS's
    // own are not.
    git_ok(&dir, &["config", "filter.lfs.clean", "git-lfs clean -- %f"]);
    assert!(git::status(&dir).unwrap().is_repo);
    for (key, value) in [
        ("filter.evil.clean", "echo ran > .git/filter-ran"),
        ("credential.helper", "!echo ran"),
        ("diff.external", "echo"),
        ("core.worktree", ".."),
    ] {
        git_ok(&dir, &["config", key, value]);
        match git::status(&dir) {
            Err(GitError::UnsafeConfig(keys)) => {
                assert_eq!(keys, [key.to_lowercase()], "{key}");
            }
            other => panic!("{key}: {other:?}"),
        }
        git_ok(&dir, &["config", "--unset", key]);
    }
    fs::write(dir.join(".gitattributes"), "*.sql filter=evil\n").unwrap();
    git_ok(&dir, &["config", "--add", "include.path", "evil.config"]);
    fs::write(
        dir.join(".git/evil.config"),
        "[filter \"evil\"]\n\tclean = echo ran > .git/filter-ran\n",
    )
    .unwrap();
    assert!(matches!(
        git::commit(&dir, &one(".gitattributes"), "attrs", None),
        Err(GitError::UnsafeConfig(_))
    ));
    assert!(!dir.join(".git/filter-ran").exists());
}

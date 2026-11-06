use std::process::Command;

fn main() {
    build_id();
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let mut attributes = tauri_build::Attributes::new();
    if msvc {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        embed_manifest();
    }
    tauri_build::try_build(attributes).expect("the Tauri build step failed");
}

/// The app's manifest, in every executable of the crate. `tauri_build` would put it in the app
/// only, and a test executable without it does not start once it links the newer functions of the
/// Common Controls: `STATUS_ENTRYPOINT_NOT_FOUND`, before any test runs.
fn embed_manifest() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("set by Cargo");
    let manifest = std::path::Path::new(&dir).join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}

/// `PLATYPUS_BUILD`: the commit the app is built from and its date, with `+` when the working
/// tree had changes, so a build can be told from another of the same version. Left unset
/// outside a git checkout (a source archive).
fn build_id() {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    // A new commit, a checkout or a staged change makes Cargo run this again.
    for path in ["HEAD", "index", "packed-refs"] {
        if let Some(p) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={p}");
        }
    }
    if let Some(p) = git(&["symbolic-ref", "-q", "HEAD"])
        .and_then(|head| git(&["rev-parse", "--git-path", &head]))
    {
        println!("cargo:rerun-if-changed={p}");
    }
    let Some(commit) = git(&["rev-parse", "--short", "HEAD"]) else {
        return;
    };
    let date =
        git(&["log", "-1", "--format=%cd", "--date=format:%Y-%m-%d %H:%M"]).unwrap_or_default();
    let dirty =
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let mark = if dirty { "+" } else { "" };
    println!("cargo:rustc-env=PLATYPUS_BUILD={commit}{mark} · {date}");
}

//! Records which source this binary was built from, for `hlt-cli version`.
//!
//! A harness copies `hlt-cli` only when it was built from a clean release tag,
//! so "dirty" counts uncommitted changes to the files that make up the binary
//! (`crates`, `Cargo.toml`, `Cargo.lock`) and leans to dirty when git cannot
//! answer.

use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let tag = git(&["describe", "--tags", "--exact-match", "HEAD"]).unwrap_or_default();
    let root = git(&["rev-parse", "--show-toplevel"]);
    let dirty = match &root {
        Some(root) => git(&[
            "-C",
            root,
            "status",
            "--porcelain",
            "--",
            "crates",
            "Cargo.toml",
            "Cargo.lock",
        ])
        .is_none_or(|changes| !changes.is_empty()),
        None => true,
    };
    println!("cargo:rustc-env=HLT_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=HLT_GIT_TAG={tag}");
    println!("cargo:rustc-env=HLT_GIT_DIRTY={dirty}");

    // Rebuild the record when HEAD, the index, tags or the sources move.
    for dir in [
        git(&["rev-parse", "--git-dir"]),
        git(&["rev-parse", "--git-common-dir"]),
    ]
    .into_iter()
    .flatten()
    {
        for name in ["HEAD", "index", "refs/tags", "packed-refs"] {
            let path = Path::new(&dir).join(name);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    if let Some(root) = root {
        for path in ["crates", "Cargo.toml", "Cargo.lock"] {
            println!(
                "cargo:rerun-if-changed={}",
                Path::new(&root).join(path).display()
            );
        }
    }
}

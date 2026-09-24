use std::path::PathBuf;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
}

fn main() {
    // A branch's HEAD file contains only its ref name; watch the ref as well so a new
    // commit rebuilds the version even when no Rust source file changed.
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            if PathBuf::from(&path).is_file() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }

    // Allow builds from source archives to identify themselves without pretending to
    // have a Git commit. Package version is always present separately in the UI.
    let revision = git(&["rev-parse", "--short=12", "HEAD"])
        .filter(|hash| hash.len() == 12 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=OXFER_GIT_REVISION={revision}");
}

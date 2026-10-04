fn main() {
    // Recompile native icon resources when the generated branding changes.
    println!("cargo:rerun-if-changed=icons");
    emit_build_info();
    tauri_build::build()
}

/// The commit and target a build came from, for `sevak --diagnostics`.
/// `SEVAK_GIT_COMMIT` in the environment wins (a source tarball has no git).
fn emit_build_info() {
    println!("cargo:rerun-if-env-changed=SEVAK_GIT_COMMIT");
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=SEVAK_BUILD_TARGET={target}");

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    let commit = std::env::var("SEVAK_GIT_COMMIT")
        .ok()
        .filter(|commit| !commit.trim().is_empty())
        .or_else(|| git(&["rev-parse", "--short=9", "HEAD"]))
        .unwrap_or_default();
    println!("cargo:rustc-env=SEVAK_GIT_COMMIT={commit}");

    // A new commit moves the branch, which the reflog records.
    if let Some(log) = git(&["rev-parse", "--git-path", "logs/HEAD"]) {
        if std::path::Path::new(&log).exists() {
            println!("cargo:rerun-if-changed={log}");
        }
    }
}

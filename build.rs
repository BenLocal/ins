use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");

    emit_git_env(
        "INS_GIT_TAG",
        &["describe", "--tags", "--exact-match", "HEAD"],
    );
    emit_git_env("INS_GIT_COMMIT", &["rev-parse", "--short=12", "HEAD"]);
    emit_git_env("INS_GIT_DIRTY", &["status", "--short"]);
}

fn emit_git_env(name: &str, args: &[&str]) {
    if let Some(value) = git_output(args) {
        let value = if name == "INS_GIT_DIRTY" {
            if value.trim().is_empty() {
                "clean".to_string()
            } else {
                "dirty".to_string()
            }
        } else {
            value
        };

        println!("cargo:rustc-env={name}={value}");
    }
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(stdout.trim().to_string())
}

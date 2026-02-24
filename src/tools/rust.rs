use std::path::Path;

use crate::runtime::process::ProcessRunner;

pub fn run_checks(process: ProcessRunner, path: &Path) -> Result<(), String> {
    run_cargo_command(process, path, &["fmt", "--all"])?;
    run_cargo_command(
        process,
        path,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run_cargo_command(process, path, &["test", "--workspace", "--all-features"])
}

fn run_cargo_command(
    process: ProcessRunner,
    path: &Path,
    cargo_args: &[&str],
) -> Result<(), String> {
    let args = cargo_command(cargo_args);
    process.run_status("nix", &args, Some(path))
}

fn cargo_command<'a>(cargo_args: &'a [&'a str]) -> Vec<&'a str> {
    let mut args = vec!["develop", "-c", "cargo"];
    args.extend_from_slice(cargo_args);
    args
}

#[cfg(test)]
mod tests {
    use super::cargo_command;

    #[test]
    fn cargo_command_uses_nix_develop_prefix() {
        let args = cargo_command(&["fmt", "--all"]);
        assert_eq!(args, vec!["develop", "-c", "cargo", "fmt", "--all"]);
    }
}

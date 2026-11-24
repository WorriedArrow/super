use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "super build and test tooling")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// fmt, clippy -D warnings, unit tests, cargo-deny
    Check,
    /// run wpt tests, fail on any result that differs from tests/wpt-meta
    Wpt {
        /// rewrite expectations from the current run
        #[arg(long)]
        update: bool,
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// tokenizer and tree construction suites
    Html5lib,
    /// launch the browser
    Run { url: Option<String> },
    /// render one page offscreen
    Headless {
        url: String,
        #[arg(long)]
        png: PathBuf,
    },
    /// screenshot every url in tests/sites.list
    Ladder,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Cmd::Check => check(),
        Cmd::Wpt { .. } => unimplemented_command("wpt"),
        Cmd::Html5lib => html5lib(),
        Cmd::Run { .. } => unimplemented_command("run"),
        Cmd::Headless { .. } => unimplemented_command("headless"),
        Cmd::Ladder => unimplemented_command("ladder"),
    }
}

fn unimplemented_command(name: &str) -> ExitCode {
    eprintln!("unimplemented: {name}");
    ExitCode::FAILURE
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

fn cargo() -> Command {
    let mut cmd = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.current_dir(workspace_root());
    cmd
}

fn run(args: &[&str]) -> bool {
    eprintln!("$ cargo {}", args.join(" "));
    match cargo().args(args).status() {
        Ok(status) => status.success(),
        Err(err) => {
            eprintln!("failed to start cargo: {err}");
            false
        }
    }
}

fn cargo_deny_installed() -> bool {
    cargo()
        .args(["deny", "--version"])
        .output()
        .is_ok_and(|out| out.status.success())
}

fn check() -> ExitCode {
    let steps: [&[&str]; 3] = [
        &["fmt", "--all", "--check"],
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        &["test", "--workspace"],
    ];
    for step in steps {
        if !run(step) {
            return ExitCode::FAILURE;
        }
    }
    if cargo_deny_installed() {
        if !run(&["deny", "check"]) {
            return ExitCode::FAILURE;
        }
    } else if env::var_os("CI").is_some() {
        eprintln!("cargo-deny is not installed, and ci must run it");
        return ExitCode::FAILURE;
    } else {
        eprintln!(
            "skipping cargo deny check: cargo-deny is not installed (cargo install cargo-deny)"
        );
    }
    ExitCode::SUCCESS
}

/// Runs the sp_html html5lib-tests harnesses, which print pass/total per file.
fn html5lib() -> ExitCode {
    if !workspace_root()
        .join("tests/html5lib-tests/tokenizer")
        .is_dir()
    {
        eprintln!(
            "tests/html5lib-tests is missing: git submodule update --init tests/html5lib-tests"
        );
        return ExitCode::FAILURE;
    }
    if run(&["test", "-p", "sp_html", "--test", "html5lib_tokenizer"]) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn agents_md_commands_parse() {
        for args in [
            &["xtask", "check"][..],
            &["xtask", "wpt", "dom/nodes"],
            &["xtask", "wpt", "--update", "dom/nodes", "css/css-box"],
            &["xtask", "html5lib"],
            &["xtask", "run"],
            &["xtask", "run", "https://example.com/"],
            &[
                "xtask",
                "headless",
                "https://example.com/",
                "--png",
                "out.png",
            ],
            &["xtask", "ladder"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok(), "{args:?}");
        }
    }

    #[test]
    fn wpt_needs_a_path() {
        assert!(Cli::try_parse_from(["xtask", "wpt"]).is_err());
    }
}

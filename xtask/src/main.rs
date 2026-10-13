//! Repository tasks: `cargo xtask ci` (fmt, clippy, tests, layers) and `cargo xtask layers`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Each crate and the internal crates it may depend on. Lower layers never reach up, and only
/// the app touches the network.
const LAYERS: &[(&str, &[&str])] = &[
    ("crates/core", &[]),
    ("crates/engine", &["remotecraft-core"]),
    ("crates/mcp", &["remotecraft-engine"]),
    ("apps/remotecraft-cli", &["remotecraft-engine", "remotecraft-mcp", "ureq"]),
];
const WATCHED: &[&str] = &["remotecraft-core", "remotecraft-engine", "remotecraft-mcp", "ureq", "reqwest", "tokio", "hyper"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_default()
}

fn layers() -> Result<(), String> {
    let mut bad = Vec::new();
    for (krate, allowed) in LAYERS {
        let manifest = root().join(krate).join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
        let mut in_deps = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_deps = line == "[dependencies]";
                continue;
            }
            let name = line.split(['=', ' ', '.']).next().unwrap_or_default();
            if in_deps && WATCHED.contains(&name) && !allowed.contains(&name) {
                bad.push(format!("{krate} must not depend on {name}"));
            }
        }
    }
    if bad.is_empty() { Ok(()) } else { Err(bad.join("\n")) }
}

fn cargo(args: &[&str]) -> Result<(), String> {
    eprintln!("$ cargo {}", args.join(" "));
    let status = Command::new(env!("CARGO")).args(args).current_dir(root()).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("cargo {} failed", args.join(" "))) }
}

fn ci() -> Result<(), String> {
    cargo(&["fmt", "--all", "--", "--check"])?;
    cargo(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    cargo(&["test", "--workspace"])?;
    layers()
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let result = match task.as_str() {
        "ci" => ci(),
        "layers" => layers(),
        _ => Err("usage: cargo xtask <ci|layers>".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

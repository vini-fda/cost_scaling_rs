//! End-to-end timing comparison between the Rust and C implementations of
//! the CS2 min-cost max-flow solver.
//!
//!
//! # Prerequisites
//!
//! - Rust toolchain (`cargo`, `rustc`)
//! - C compiler (`gcc` or `cc`) and `make`
//! - [`hyperfine`](https://github.com/sharkdp/hyperfine):
//!   `cargo install hyperfine` or `brew install hyperfine`
//!
//! # Usage
//!
//! ```sh
//! cargo run --release --example bench_compare
//! cargo run --release --example bench_compare -- --warmup 5
//! ```
//!
//! Extra arguments after `--` are forwarded directly to `hyperfine`.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::{env, fs};

/// Project root resolved at compile time so the example works regardless of
/// the working directory from which it is invoked.
const PROJECT_ROOT: &str = env!("CARGO_MANIFEST_DIR");

const RUST_BIN: &str = "target/release/cost-scaling-rs";
const C_BIN: &str = "cs2/cs2";
const DATA_DIR: &str = "target/benchdata";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Anchor all relative paths to the project root.
    env::set_current_dir(PROJECT_ROOT)?;

    // Arguments after `--` are forwarded verbatim to hyperfine.
    let extra_args: Vec<String> = env::args().skip(1).collect();

    // Build the full hyperfine argument list: defaults first, then user extras.
    let default_args = ["--warmup", "3", "--min-runs", "20"];
    let hyperfine_base: Vec<&str> = default_args
        .iter()
        .copied()
        .chain(extra_args.iter().map(String::as_str))
        .collect();

    // -----------------------------------------------------------------------
    // Preflight checks
    // -----------------------------------------------------------------------
    check_dep("cargo", "Install from https://rustup.rs")?;
    check_dep(
        "make",
        "Install build-essential (Linux) or Xcode CLI tools (macOS)",
    )?;
    check_dep(
        "hyperfine",
        "Install: cargo install hyperfine  OR  brew install hyperfine",
    )?;

    // -----------------------------------------------------------------------
    // Build
    // -----------------------------------------------------------------------
    println!("==> Building Rust binary (release, LTO)...");
    run("cargo", &["build", "--release", "--quiet"])?;

    println!("==> Building C binary (release, -O3 -march=native -flto)...");
    // `make clean` is best-effort — ignore failure and suppress its output.
    let _ = Command::new("make")
        .args(["-C", "cs2", "clean", "--quiet"])
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status();
    run_silent_stderr("make", &["-C", "cs2", "release", "--quiet"])?;

    // Verify the binaries were actually produced.
    if !PathBuf::from(RUST_BIN).is_file() {
        return Err(format!("Rust binary not found at {RUST_BIN}").into());
    }
    if !PathBuf::from(C_BIN).is_file() {
        return Err(format!("C binary not found at {C_BIN}").into());
    }

    // -----------------------------------------------------------------------
    // Generate test data
    // -----------------------------------------------------------------------
    println!("==> Generating GOTO test problems...");
    run(
        "cargo",
        &[
            "run",
            "--release",
            "--quiet",
            "--example",
            "gen_goto",
            "--",
            DATA_DIR,
        ],
    )?;

    // -----------------------------------------------------------------------
    // Run benchmarks
    // -----------------------------------------------------------------------
    println!();
    println!("======================================================================");
    println!("  Rust ({RUST_BIN}) vs C ({C_BIN})");
    println!("  hyperfine args: {}", hyperfine_base.join(" "));
    println!("======================================================================");
    println!();

    let mut problems: Vec<PathBuf> = fs::read_dir(DATA_DIR)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("min")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("goto_"))
        })
        .collect();
    problems.sort();

    for problem in &problems {
        let stem = problem
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("(unknown)");
        println!("--- {stem} ---");

        // hyperfine runs each command string through a shell, so the
        // `> /dev/null` and `< file` redirections are interpreted correctly.
        let rust_cmd = format!("{RUST_BIN} {} > /dev/null", problem.display());
        let c_cmd = format!("{C_BIN} < {} > /dev/null", problem.display());

        let mut args: Vec<&str> = hyperfine_base.clone();
        args.extend_from_slice(&["--command-name", "Rust", rust_cmd.as_str()]);
        args.extend_from_slice(&["--command-name", "C", c_cmd.as_str()]);

        run("hyperfine", &args)?;
        println!();
    }

    Ok(())
}

/// Returns an error if `name` is not found in PATH.
fn check_dep(name: &str, hint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let found = Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if found {
        Ok(())
    } else {
        Err(format!("'{name}' is required but not found.\n  {hint}").into())
    }
}

/// Runs `program args`, inheriting stdio. Fails if the process exits non-zero.
fn run(program: &str, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new(program).args(args).status()?;
    if !status.success() {
        return Err(format!(
            "command failed (exit {}): {} {}",
            status.code().unwrap_or(-1),
            program,
            args.join(" ")
        )
        .into());
    }
    Ok(())
}

/// Like [`run`] but discards stderr (equivalent to `2>/dev/null`).
fn run_silent_stderr(program: &str, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new(program)
        .args(args)
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(format!(
            "command failed (exit {}): {} {}",
            status.code().unwrap_or(-1),
            program,
            args.join(" ")
        )
        .into());
    }
    Ok(())
}

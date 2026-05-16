//! CPU profiling with samply (opens Firefox Profiler in browser).
//!
//! # Prerequisites
//!
//! ```sh
//! cargo install --locked samply
//! ```
//!
//! # Usage
//!
//! ```sh
//! cargo run --example profile
//! cargo run --example profile -- --save-only
//! cargo run --example profile -- --iterations 50
//! cargo run --example profile -- --size 5000
//! ```

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::{env, fs};

/// Project root resolved at compile time so the example works regardless of
/// the working directory from which it is invoked.
const PROJECT_ROOT: &str = env!("CARGO_MANIFEST_DIR");

const DATA_DIR: &str = "target/benchdata";
const PROFILING_BIN: &str = "target/profiling/cost-scaling-rs";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Anchor all relative paths to the project root.
    env::set_current_dir(PROJECT_ROOT)?;

    // -----------------------------------------------------------------------
    // Argument parsing
    // -----------------------------------------------------------------------
    let mut iterations: u32 = 20;
    let mut problem_size = String::from("10000");
    let mut save_only = false;

    let raw_args: Vec<String> = env::args().skip(1).collect();
    let mut i = 0;
    while i < raw_args.len() {
        match raw_args[i].as_str() {
            "--save-only" => {
                save_only = true;
                i += 1;
            }
            "--iterations" => {
                i += 1;
                let val = raw_args.get(i).ok_or("--iterations requires a value")?;
                iterations = val
                    .parse()
                    .map_err(|_| format!("invalid iteration count: {val}"))?;
                i += 1;
            }
            "--size" => {
                i += 1;
                problem_size.clone_from(raw_args.get(i).ok_or("--size requires a value")?);
                i += 1;
            }
            other => {
                eprintln!("Unknown option: {other}");
                eprintln!(
                    "Usage: cargo run --example profile \
                     -- [--save-only] [--iterations N] [--size NODES]"
                );
                return Err("unknown option".into());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Preflight check
    // -----------------------------------------------------------------------
    check_dep("samply", "Install with: cargo install --locked samply")?;

    // -----------------------------------------------------------------------
    // Build with debug symbols (release optimizations, no stripping)
    // -----------------------------------------------------------------------
    println!("==> Building with profiling profile...");
    run("cargo", &["build", "--profile", "profiling", "--quiet"])?;

    // -----------------------------------------------------------------------
    // Generate test data if the benchdata directory is absent or empty
    // -----------------------------------------------------------------------
    let data_dir = PathBuf::from(DATA_DIR);
    let has_data = data_dir.is_dir()
        && fs::read_dir(&data_dir)?
            .filter_map(Result::ok)
            .any(|e| e.path().extension().and_then(OsStr::to_str) == Some("min"));

    if !has_data {
        println!("==> Generating test problems...");
        fs::create_dir_all(&data_dir)?;
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
    }

    // -----------------------------------------------------------------------
    // Find the problem file matching the requested size
    // -----------------------------------------------------------------------
    let prefix = format!("goto_{problem_size}n_");
    let problem = fs::read_dir(&data_dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.extension().and_then(OsStr::to_str) == Some("min")
                && p.file_name()
                    .and_then(OsStr::to_str)
                    .is_some_and(|n| n.starts_with(&prefix))
        });

    let Some(problem) = problem else {
        eprintln!("Error: No problem file found for size {problem_size}.");
        eprintln!("Available:");
        let mut available: Vec<PathBuf> = fs::read_dir(&data_dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(OsStr::to_str) == Some("min"))
            .collect();
        available.sort();
        for p in &available {
            eprintln!("  {}", p.display());
        }
        return Err(format!("no problem file for size {problem_size}").into());
    };

    let filename = problem
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("(unknown)");
    let total_secs = iterations * 300 / 1000;
    println!("==> Profiling {iterations} iterations of {filename}...");
    println!("    (Each run takes ~300ms, total ~{total_secs}s)");
    println!();

    // -----------------------------------------------------------------------
    // Run samply
    // -----------------------------------------------------------------------
    let problem_str = problem.to_string_lossy().into_owned();
    let iterations_str = iterations.to_string();

    let mut samply_args: Vec<&str> = vec!["record", "--iteration-count", &iterations_str];
    if save_only {
        samply_args.extend_from_slice(&["--save-only", "-o", "profile.json"]);
    }
    samply_args.extend_from_slice(&[PROFILING_BIN, &problem_str]);

    run("samply", &samply_args)?;

    if save_only {
        println!();
        println!("==> Profile saved to profile.json");
        println!("    View at: https://profiler.firefox.com/ (click Load...)");
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

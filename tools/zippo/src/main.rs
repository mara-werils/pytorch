mod collector;

use std::path::PathBuf;
use std::process;
use std::time::Instant;

use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "zippo", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Collect tests from the given directories
    Collect {
        /// Files or directories to collect tests from
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Number of parallel jobs (default: min(num_cpus, 16))
        #[arg(short, long, default_value_t = default_jobs())]
        jobs: usize,

        /// Repository root (for computing relative paths)
        #[arg(long, default_value = ".")]
        repo_root: PathBuf,
    },
}

#[derive(Serialize)]
struct Output {
    tests: std::collections::BTreeMap<String, Vec<String>>,
}

fn default_jobs() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    cpus.min(16)
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Collect {
            paths,
            jobs,
            repo_root,
        } => {
            let repo_root = match repo_root.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("error: invalid repo root '{}': {}", repo_root.display(), e);
                    process::exit(1);
                }
            };

            let abs_paths: Vec<PathBuf> = paths
                .iter()
                .map(|p| {
                    if p.is_absolute() {
                        p.clone()
                    } else {
                        repo_root.join(p)
                    }
                })
                .collect();

            let start = Instant::now();

            let result = match collector::collect(&abs_paths, &repo_root, jobs) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("error: {}", e);
                    process::exit(1);
                }
            };

            let elapsed = start.elapsed();

            for err in &result.errors {
                eprintln!("warning: {}", err);
            }

            let test_count: usize = result.tests.values().map(|v| v.len()).sum();
            let file_count = result.tests.len();
            eprintln!(
                "{} tests collected from {} files in {:.2}s",
                test_count,
                file_count,
                elapsed.as_secs_f64()
            );

            let output = Output {
                tests: result.tests,
            };
            println!("{}", serde_json::to_string(&output).unwrap());
        }
    }
}

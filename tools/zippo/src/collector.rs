use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use serde::Deserialize;
use walkdir::WalkDir;

const COLLECT_SCRIPT: &str = include_str!("collect.py");

pub struct CollectResult {
    pub tests: BTreeMap<String, Vec<String>>,
    pub errors: Vec<String>,
}

#[derive(Deserialize)]
struct WorkerOutput {
    results: Vec<FileResult>,
}

#[derive(Deserialize)]
struct FileResult {
    file: String,
    tests: Vec<String>,
    error: Option<String>,
}

pub fn find_test_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            files.push(path.clone());
        } else if path.is_dir() {
            for entry in WalkDir::new(path).follow_links(false) {
                let entry = entry.with_context(|| format!("walking {}", path.display()))?;
                if entry.file_type().is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.starts_with("test_") && name.ends_with(".py") {
                            files.push(entry.into_path());
                        }
                    }
                }
            }
        } else {
            anyhow::bail!("{} is not a file or directory", path.display());
        }
    }
    files.sort();
    Ok(files)
}

fn find_python(repo_root: &Path) -> Result<PathBuf> {
    let venv_python = repo_root.join(".venv/bin/python");
    if venv_python.exists() {
        return Ok(venv_python);
    }
    for name in ["python3", "python"] {
        if let Ok(output) = Command::new("which").arg(name).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Ok(PathBuf::from(path));
            }
        }
    }
    anyhow::bail!(
        "could not find python. expected .venv/bin/python in repo root or python3/python on PATH"
    )
}

pub fn collect(paths: &[PathBuf], repo_root: &Path, jobs: usize) -> Result<CollectResult> {
    let files = find_test_files(paths)?;
    let python = find_python(repo_root)?;

    let file_strs: Vec<String> = files
        .iter()
        .map(|f| f.to_string_lossy().into_owned())
        .collect();

    let output_path = "/tmp/zippo_output.json";
    let input = serde_json::json!({
        "files": file_strs,
        "repo_root": repo_root.to_string_lossy(),
        "jobs": jobs,
        "output": output_path,
    });

    let script_path = "/tmp/zippo_collect.py";
    std::fs::write(script_path, COLLECT_SCRIPT)
        .context("failed to write collector script")?;

    let mut child = Command::new(&python)
        .arg(script_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .current_dir("/tmp")
        .spawn()
        .context("failed to spawn python worker")?;

    let stdin = child.stdin.as_mut().context("failed to open stdin")?;
    serde_json::to_writer(stdin, &input).context("failed to write input")?;
    drop(child.stdin.take());

    let status = child.wait().context("failed to wait for python worker")?;
    if !status.success() {
        anyhow::bail!("python worker exited with {}", status);
    }

    let output_bytes =
        std::fs::read(output_path).context("failed to read worker output file")?;
    let _ = std::fs::remove_file(output_path);

    let worker_output: WorkerOutput =
        serde_json::from_slice(&output_bytes).context("failed to parse worker output")?;

    let mut tests = BTreeMap::new();
    let mut errors = Vec::new();

    for result in worker_output.results {
        if let Some(err) = result.error {
            errors.push(format!("{}: {}", result.file, err));
        }
        if !result.tests.is_empty() {
            tests.insert(result.file, result.tests);
        }
    }

    Ok(CollectResult { tests, errors })
}

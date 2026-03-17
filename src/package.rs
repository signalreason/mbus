use crate::bench::{BenchCostSummary, BenchGate, BenchReport, BenchTokenUsage};
use crate::output::OutputArtifact;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::FileOptions;

pub const CHALLENGE_PACKAGE_SCHEMA_VERSION: u32 = 1;
pub const PACKAGE_ROOT_DIR: &str = "target/challenge/package";
pub const PACKAGED_REPORT_FILENAME: &str = "report.json";
pub const PACKAGED_MANIFEST_FILENAME: &str = "manifest.json";
pub const PACKAGED_README_FILENAME: &str = "README.md";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChallengePackageManifest {
    pub schema_version: u32,
    pub generated_at: String,
    pub source_report_path: String,
    pub report_timestamp: String,
    pub gate: BenchGate,
    pub aggregate_usage: BenchTokenUsage,
    pub aggregate_cost: BenchCostSummary,
    pub files: Vec<PackagedFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackagedFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageOptions {
    pub report_path: PathBuf,
    pub output_dir: PathBuf,
    pub zip_path: PathBuf,
    pub overwrite: bool,
}

#[derive(Clone, Debug)]
pub struct PackageOutput {
    pub bundle_dir: PathBuf,
    pub zip_path: PathBuf,
    pub manifest: ChallengePackageManifest,
}

pub fn default_output_dir(report_path: &Path) -> Result<PathBuf, String> {
    let stem = report_stem(report_path)?;
    Ok(PathBuf::from(PACKAGE_ROOT_DIR).join(stem))
}

pub fn default_zip_path(report_path: &Path) -> Result<PathBuf, String> {
    let stem = report_stem(report_path)?;
    Ok(PathBuf::from(PACKAGE_ROOT_DIR).join(format!("{stem}.zip")))
}

pub fn package_challenge_report(options: &PackageOptions) -> Result<PackageOutput, String> {
    let report_bytes = fs::read(&options.report_path).map_err(|err| {
        format!(
            "failed to read report {}: {err}",
            options.report_path.display()
        )
    })?;
    let report: BenchReport = serde_json::from_slice(&report_bytes).map_err(|err| {
        format!(
            "failed to parse challenge report {}: {err}",
            options.report_path.display()
        )
    })?;
    validate_challenge_report(&report)?;
    prepare_destinations(&options.output_dir, &options.zip_path, options.overwrite)?;

    fs::create_dir_all(&options.output_dir).map_err(|err| {
        format!(
            "failed to create package directory {}: {err}",
            options.output_dir.display()
        )
    })?;

    let mut inventory = Vec::new();
    let report_destination = options.output_dir.join(PACKAGED_REPORT_FILENAME);
    write_bytes(&report_destination, &report_bytes)?;
    inventory.push(file_inventory_entry(
        &options.output_dir,
        &report_destination,
        &report_bytes,
    )?);

    let readme_bytes = build_reproduction_readme(&report).into_bytes();
    let readme_destination = options.output_dir.join(PACKAGED_README_FILENAME);
    write_bytes(&readme_destination, &readme_bytes)?;
    inventory.push(file_inventory_entry(
        &options.output_dir,
        &readme_destination,
        &readme_bytes,
    )?);

    let mut seen_destinations = HashSet::new();
    for result in &report.results {
        for artifact in &result.output_artifacts {
            let source_path = PathBuf::from(&artifact.path);
            let bytes = read_and_validate_artifact(&source_path, artifact)?;
            let relative_path = packaged_artifact_relative_path(&result.task_id, &source_path)?;
            if !seen_destinations.insert(relative_path.clone()) {
                return Err(format!(
                    "duplicate packaged artifact path: {}",
                    portable_path(&relative_path)
                ));
            }
            let destination = options.output_dir.join(&relative_path);
            write_bytes(&destination, &bytes)?;
            inventory.push(file_inventory_entry(
                &options.output_dir,
                &destination,
                &bytes,
            )?);
        }
    }

    inventory.sort_by(|left, right| left.path.cmp(&right.path));
    let manifest = ChallengePackageManifest {
        schema_version: CHALLENGE_PACKAGE_SCHEMA_VERSION,
        generated_at: crate::bench::now_timestamp()
            .map_err(|err| format!("package timestamp: {err}"))?,
        source_report_path: canonical_display_path(&options.report_path),
        report_timestamp: report.timestamp.clone(),
        gate: report.gate.clone(),
        aggregate_usage: report.aggregate_usage.clone(),
        aggregate_cost: report.aggregate_cost.clone(),
        // Exclude manifest.json from its own checksum inventory to avoid self-referential hashes.
        files: inventory,
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|err| format!("serialize manifest: {err}"))?;
    let manifest_destination = options.output_dir.join(PACKAGED_MANIFEST_FILENAME);
    write_bytes(&manifest_destination, &manifest_bytes)?;

    write_zip_archive(&options.output_dir, &options.zip_path)?;

    Ok(PackageOutput {
        bundle_dir: options.output_dir.clone(),
        zip_path: options.zip_path.clone(),
        manifest,
    })
}

fn report_stem(report_path: &Path) -> Result<String, String> {
    report_path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("report path {} has no file stem", report_path.display()))
}

fn validate_challenge_report(report: &BenchReport) -> Result<(), String> {
    if report.results.is_empty() {
        return Err("report has no results".to_string());
    }

    let normalized_tasks_dir = report.tasks_dir.replace('\\', "/");
    let looks_like_challenge_dir = normalized_tasks_dir
        .split('/')
        .any(|component| component == "challenge");
    let looks_like_challenge_tasks = report
        .results
        .iter()
        .all(|result| result.task_id.starts_with("challenge-"));

    if !(looks_like_challenge_dir || looks_like_challenge_tasks) {
        return Err(format!(
            "report does not look like a challenge report: tasks_dir={}",
            report.tasks_dir
        ));
    }

    Ok(())
}

fn prepare_destinations(output_dir: &Path, zip_path: &Path, overwrite: bool) -> Result<(), String> {
    if output_dir.exists() {
        if !overwrite {
            return Err(format!(
                "package directory already exists: {} (pass --overwrite to replace it)",
                output_dir.display()
            ));
        }
        fs::remove_dir_all(output_dir).map_err(|err| {
            format!(
                "failed to remove package directory {}: {err}",
                output_dir.display()
            )
        })?;
    }

    if zip_path.exists() {
        if !overwrite {
            return Err(format!(
                "package archive already exists: {} (pass --overwrite to replace it)",
                zip_path.display()
            ));
        }
        fs::remove_file(zip_path)
            .map_err(|err| format!("failed to remove archive {}: {err}", zip_path.display()))?;
    }

    Ok(())
}

fn read_and_validate_artifact(path: &Path, artifact: &OutputArtifact) -> Result<Vec<u8>, String> {
    let bytes = fs::read(path)
        .map_err(|err| format!("failed to read artifact {}: {err}", path.display()))?;
    if let Some(expected_sha) = artifact.sha256.as_deref() {
        let actual_sha = sha256_hex(&bytes);
        if actual_sha != expected_sha {
            return Err(format!(
                "artifact checksum mismatch for {}: expected {}, got {}",
                path.display(),
                expected_sha,
                actual_sha
            ));
        }
    }
    Ok(bytes)
}

fn packaged_artifact_relative_path(task_id: &str, source_path: &Path) -> Result<PathBuf, String> {
    let tail = strip_ralph_runs_prefix(source_path).unwrap_or_else(|| {
        source_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("artifact.bin"))
    });
    if tail.as_os_str().is_empty() {
        return Err(format!(
            "artifact path {} does not resolve to a bundle path",
            source_path.display()
        ));
    }
    Ok(PathBuf::from("artifacts").join(task_id).join(tail))
}

fn strip_ralph_runs_prefix(path: &Path) -> Option<PathBuf> {
    let components: Vec<_> = path.components().collect();
    for index in 0..components.len() {
        let current = components.get(index)?;
        let next = components.get(index + 1)?;
        if component_str(*current) == Some(".ralph") && component_str(*next) == Some("runs") {
            let mut output = PathBuf::new();
            for component in components.iter().skip(index + 2) {
                if let Component::Normal(value) = component {
                    output.push(value);
                }
            }
            return Some(output);
        }
    }
    None
}

fn component_str(component: Component<'_>) -> Option<&str> {
    match component {
        Component::Normal(value) => value.to_str(),
        _ => None,
    }
}

fn build_reproduction_readme(report: &BenchReport) -> String {
    let mut command_lines = vec![
        "MBUS_LLM_API_KEY=... cargo run -- challenge \\".to_string(),
        format!("  --report-path {} \\", report.report_path),
        format!("  --required-passes {} \\", report.required_passes),
        format!("  --max-steps-per-task {} \\", report.max_steps_per_task),
        "  --headless true \\".to_string(),
        format!("  --llm-model-fast {} \\", report.llm.model_fast),
        format!("  --llm-model-mid {} \\", report.llm.model_mid),
        format!("  --llm-model-strong {}", report.llm.model_strong),
    ];
    if let Some(pricing) = report.aggregate_cost.pricing {
        let last = command_lines
            .pop()
            .unwrap_or_else(|| format!("  --llm-model-strong {}", report.llm.model_strong));
        command_lines.push(format!("{last} \\"));
        command_lines.push(format!(
            "  --llm-input-cost-per-million {} \\",
            pricing.input_cost_per_million
        ));
        command_lines.push(format!(
            "  --llm-output-cost-per-million {}",
            pricing.output_cost_per_million
        ));
    }

    format!(
        "# Challenge Package\n\n\
This bundle was generated from an existing `mbus challenge` report.\n\n\
## Prerequisites\n\n\
- Rust toolchain with `cargo`\n\
- A Chromium or Chrome binary discoverable by `chromiumoxide`\n\
- An OpenAI-compatible API key exported as `MBUS_LLM_API_KEY`\n\n\
## Reproduction\n\n\
Run the command below from the repository root:\n\n\
```bash\n{}\n```\n\n\
The packaged `report.json` captures the original gate result, token usage, and cost summary from the source run.\n",
        command_lines.join("\n")
    )
}

fn canonical_display_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn file_inventory_entry(root: &Path, path: &Path, bytes: &[u8]) -> Result<PackagedFile, String> {
    let relative = path.strip_prefix(root).map_err(|err| {
        format!(
            "package path {} not under {}: {err}",
            path.display(),
            root.display()
        )
    })?;
    Ok(PackagedFile {
        path: portable_path(relative),
        bytes: bytes.len() as u64,
        sha256: sha256_hex(bytes),
    })
}

fn portable_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn write_zip_archive(bundle_dir: &Path, zip_path: &Path) -> Result<(), String> {
    if let Some(parent) = zip_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }

    let file = fs::File::create(zip_path)
        .map_err(|err| format!("failed to create archive {}: {err}", zip_path.display()))?;
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut files = Vec::new();
    collect_files(bundle_dir, bundle_dir, &mut files)?;
    files.sort();
    for relative_path in files {
        let source_path = bundle_dir.join(&relative_path);
        let mut source = fs::File::open(&source_path)
            .map_err(|err| format!("failed to open {}: {err}", source_path.display()))?;
        writer
            .start_file(portable_path(&relative_path), options)
            .map_err(|err| format!("failed to add {} to archive: {err}", source_path.display()))?;
        let mut buffer = Vec::new();
        source
            .read_to_end(&mut buffer)
            .map_err(|err| format!("failed to read {}: {err}", source_path.display()))?;
        writer.write_all(&buffer).map_err(|err| {
            format!(
                "failed to write {} to archive: {err}",
                source_path.display()
            )
        })?;
    }
    writer
        .finish()
        .map_err(|err| format!("failed to finalize archive {}: {err}", zip_path.display()))?;
    Ok(())
}

fn collect_files(root: &Path, current: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(current)
        .map_err(|err| {
            format!(
                "failed to read package directory {}: {err}",
                current.display()
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            format!(
                "failed to iterate package directory {}: {err}",
                current.display()
            )
        })?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("failed to inspect {}: {err}", path.display()))?;
        if file_type.is_dir() {
            collect_files(root, &path, output)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|err| {
                format!(
                    "package path {} not under {}: {err}",
                    path.display(),
                    root.display()
                )
            })?;
            output.push(relative.to_path_buf());
        }
    }

    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{
        BENCH_REPORT_SCHEMA_VERSION, BenchLlmInfo, BenchObservedStatus, BenchTaskResult,
    };
    use tempfile::tempdir;

    fn sample_report() -> BenchReport {
        BenchReport {
            schema_version: BENCH_REPORT_SCHEMA_VERSION,
            timestamp: "2026-03-17T00:00:00Z".to_string(),
            tasks_dir: "harness/challenge".to_string(),
            report_path: "target/challenge/report.json".to_string(),
            llm: BenchLlmInfo {
                mode: "openai".to_string(),
                model_fast: "gpt-5-mini".to_string(),
                model_mid: "gpt-5.1".to_string(),
                model_strong: "gpt-5.2".to_string(),
            },
            max_steps_per_task: 40,
            required_passes: 10,
            duration_ms: 1000,
            gate: BenchGate {
                total_tasks: 12,
                passed_tasks: 12,
                required_passes: 10,
                passed: true,
                reason: None,
            },
            summary: crate::bench::BenchSummary {
                total_tasks: 12,
                passed_tasks: 12,
                required_passes: 10,
                completion_rate: 1.0,
                median_steps_success: Some(2),
                p95_steps_success: Some(3),
                gate_passed: true,
            },
            aggregate_usage: BenchTokenUsage {
                prompt_tokens: Some(100),
                completion_tokens: Some(20),
                total_tokens: Some(120),
                error: None,
            },
            aggregate_cost: BenchCostSummary {
                pricing: Some(crate::bench::BenchPricing {
                    input_cost_per_million: 1.0,
                    output_cost_per_million: 2.0,
                }),
                input_cost_usd: Some(0.001),
                output_cost_usd: Some(0.002),
                total_cost_usd: Some(0.003),
                error: None,
            },
            failure_buckets: Default::default(),
            results: vec![BenchTaskResult {
                task_id: "challenge-01".to_string(),
                passed: true,
                status: BenchObservedStatus::Done,
                steps: 2,
                duration_ms: 200,
                usage: BenchTokenUsage {
                    prompt_tokens: Some(10),
                    completion_tokens: Some(5),
                    total_tokens: Some(15),
                    error: None,
                },
                failure_reason: None,
                output_artifacts: Vec::new(),
                final_url: Some("http://127.0.0.1/challenge/cookie-banner.html".to_string()),
                final_visible_text: Some("COOKIE BANNER DISMISSED".to_string()),
            }],
        }
    }

    #[test]
    fn validate_artifact_checksum_matches_report() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("artifact.txt");
        let bytes = b"artifact-bytes";
        fs::write(&path, bytes).expect("write artifact");
        let artifact = OutputArtifact {
            kind: "screenshot".to_string(),
            path: path.display().to_string(),
            record_count: None,
            step_index: Some(1),
            artifact_ref: None,
            mime_type: Some("image/png".to_string()),
            sha256: Some(sha256_hex(bytes)),
            bytes: Some(bytes.len()),
        };

        let validated = read_and_validate_artifact(&path, &artifact).expect("validated");
        assert_eq!(validated, bytes);
    }

    #[test]
    fn packaged_artifact_paths_are_relative_and_stable() {
        let path = Path::new(".ralph/runs/task_1_2026/steps/step-1/screenshot.png");

        let relative = packaged_artifact_relative_path("challenge-01", path).expect("relative");

        assert_eq!(
            portable_path(&relative),
            "artifacts/challenge-01/task_1_2026/steps/step-1/screenshot.png"
        );
    }

    #[test]
    fn manifest_inventory_uses_relative_paths() {
        let dir = tempdir().expect("tempdir");
        let file_path = dir.path().join("artifacts/challenge-01/report.txt");
        let bytes = b"report";
        write_bytes(&file_path, bytes).expect("write file");

        let entry = file_inventory_entry(dir.path(), &file_path, bytes).expect("entry");

        assert_eq!(entry.path, "artifacts/challenge-01/report.txt");
        assert_eq!(entry.bytes, 6);
        assert_eq!(entry.sha256, sha256_hex(bytes));
    }

    #[test]
    fn prepare_destinations_requires_overwrite() {
        let dir = tempdir().expect("tempdir");
        let output_dir = dir.path().join("bundle");
        let zip_path = dir.path().join("bundle.zip");
        fs::create_dir_all(&output_dir).expect("create bundle dir");
        fs::write(&zip_path, b"zip").expect("write zip");

        let err = prepare_destinations(&output_dir, &zip_path, false).expect_err("expected error");
        assert!(err.contains("already exists"));

        prepare_destinations(&output_dir, &zip_path, true).expect("overwrite");
        assert!(!output_dir.exists());
        assert!(!zip_path.exists());
    }

    #[test]
    fn build_reproduction_readme_includes_models_and_pricing() {
        let report = sample_report();

        let readme = build_reproduction_readme(&report);

        assert!(readme.contains("cargo run -- challenge"));
        assert!(readme.contains("--llm-model-fast gpt-5-mini"));
        assert!(readme.contains("--llm-input-cost-per-million 1"));
        assert!(readme.contains("--llm-output-cost-per-million 2"));
    }
}

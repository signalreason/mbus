use crate::output::sha256_hex;
use clap::Args;
use image::load_from_memory;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

const VISUAL_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Args, Debug)]
pub struct VisualArgs {
    /// Path to the baseline run artifact directory (e.g. .ralph/runs/<run_id>)
    #[arg(long, value_name = "PATH")]
    pub baseline: PathBuf,

    /// Path to the candidate run artifact directory (e.g. .ralph/runs/<run_id>)
    #[arg(long, value_name = "PATH")]
    pub candidate: PathBuf,

    /// Path where the generated visual report will be written
    #[arg(long, value_name = "PATH", default_value = "visual-report.json")]
    pub report: PathBuf,
}

/// Entrypoint for the visual evaluator utility.
pub fn run_command(args: VisualArgs) -> Result<(), Box<dyn Error>> {
    let baseline = RunArtifacts::load(&args.baseline)?;
    let candidate = RunArtifacts::load(&args.candidate)?;
    let report = VisualReport::from_runs(&baseline, &candidate);
    write_report(&report, &args.report)?;
    Ok(())
}

fn write_report(report: &VisualReport, path: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(path)?;
    serde_json::to_writer_pretty(file, report)?;
    Ok(())
}

#[derive(Debug)]
struct RunArtifacts {
    run_id: String,
    steps: Vec<StepScreenshot>,
}

impl RunArtifacts {
    fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        let run_id = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("run directory requires a valid name")?
            .to_string();
        let steps_dir = path.join("steps");
        let mut steps = Vec::new();
        for entry in fs::read_dir(&steps_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = match name.to_str() {
                Some(value) => value,
                None => continue,
            };
            let Some(index_str) = name.strip_prefix("step-") else {
                continue;
            };
            let step_index = match index_str.parse::<usize>() {
                Ok(value) => value,
                Err(_) => continue,
            };
            let screenshot_path = entry.path().join("screenshot.png");
            if !screenshot_path.is_file() {
                continue;
            }
            let bytes = fs::read(&screenshot_path)?;
            let sha256 = sha256_hex(&bytes);
            steps.push(StepScreenshot {
                step_index,
                path: screenshot_path,
                bytes,
                sha256,
            });
        }
        steps.sort_by_key(|value| value.step_index);
        Ok(Self { run_id, steps })
    }
}

#[derive(Debug)]
struct StepScreenshot {
    step_index: usize,
    path: PathBuf,
    bytes: Vec<u8>,
    sha256: String,
}

#[derive(Serialize)]
struct VisualReport {
    schema_version: u32,
    generated_at: String,
    baseline: RunSummary,
    candidate: RunSummary,
    comparisons: Vec<ComparisonSummary>,
    ocr_entries: Vec<OcrEntry>,
}

impl VisualReport {
    fn from_runs(baseline: &RunArtifacts, candidate: &RunArtifacts) -> Self {
        Self {
            schema_version: VISUAL_REPORT_SCHEMA_VERSION,
            generated_at: OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
            baseline: RunSummary::from_artifacts(baseline),
            candidate: RunSummary::from_artifacts(candidate),
            comparisons: ComparisonSummary::between(baseline, candidate),
            ocr_entries: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct RunSummary {
    run_id: String,
    steps: Vec<StepSummary>,
}

impl RunSummary {
    fn from_artifacts(value: &RunArtifacts) -> Self {
        Self {
            run_id: value.run_id.clone(),
            steps: value
                .steps
                .iter()
                .map(|step| StepSummary {
                    step_index: step.step_index,
                    path: step.path.display().to_string(),
                    sha256: step.sha256.clone(),
                    bytes: step.bytes.len(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct StepSummary {
    step_index: usize,
    path: String,
    sha256: String,
    bytes: usize,
}

#[derive(Serialize)]
struct ComparisonSummary {
    step_index: usize,
    baseline_sha256: String,
    candidate_sha256: String,
    size_delta: i64,
    score: f64,
    changed_regions: Vec<ChangedRegion>,
}

impl ComparisonSummary {
    fn between(baseline: &RunArtifacts, candidate: &RunArtifacts) -> Vec<Self> {
        let candidate_map: BTreeMap<_, _> = candidate
            .steps
            .iter()
            .map(|step| (step.step_index, step))
            .collect();
        let mut comparisons = Vec::new();
        for base in &baseline.steps {
            if let Some(cand) = candidate_map.get(&base.step_index) {
                let size_delta = cand.bytes.len() as i64 - base.bytes.len() as i64;
                let denom = base.bytes.len().max(cand.bytes.len());
                let score = if denom == 0 {
                    0.0
                } else {
                    (size_delta.abs() as f64) / (denom as f64)
                };
                let changed_regions = changed_regions_from_bytes(&base.bytes, &cand.bytes);
                comparisons.push(Self {
                    step_index: base.step_index,
                    baseline_sha256: base.sha256.clone(),
                    candidate_sha256: cand.sha256.clone(),
                    size_delta,
                    score,
                    changed_regions,
                });
            }
        }
        comparisons
    }
}

#[derive(Serialize)]
struct ChangedRegion {
    bbox: [u32; 4],
    score: f64,
    pixels: usize,
}

fn changed_regions_from_bytes(base: &[u8], candidate: &[u8]) -> Vec<ChangedRegion> {
    const PIXEL_DIFF_THRESHOLD: u32 = 30;
    const MAX_PIXEL_DIFF: f64 = 255.0 * 3.0;

    let base_img = match load_from_memory(base) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return Vec::new(),
    };
    let candidate_img = match load_from_memory(candidate) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return Vec::new(),
    };

    if base_img.dimensions() != candidate_img.dimensions() {
        return Vec::new();
    }

    let (width, height) = base_img.dimensions();
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let mut diff_map = Vec::with_capacity(width_usize * height_usize);

    for y in 0..height {
        for x in 0..width {
            let before = base_img.get_pixel(x, y).0;
            let after = candidate_img.get_pixel(x, y).0;
            let pixel_diff: u32 = before[..3]
                .iter()
                .zip(after[..3].iter())
                .map(|(b, a)| (*b as i16 - *a as i16).unsigned_abs() as u32)
                .sum();
            diff_map.push(pixel_diff);
        }
    }

    let mut visited = vec![false; diff_map.len()];
    let mut regions = Vec::new();

    for y in 0..height_usize {
        for x in 0..width_usize {
            let idx = y * width_usize + x;
            if visited[idx] || diff_map[idx] < PIXEL_DIFF_THRESHOLD {
                continue;
            }

            let mut queue = VecDeque::new();
            visited[idx] = true;
            queue.push_back(idx);

            let mut min_x = x;
            let mut min_y = y;
            let mut max_x = x;
            let mut max_y = y;
            let mut sum_diff = 0u64;
            let mut pixel_count = 0usize;

            while let Some(current) = queue.pop_front() {
                let cy = current / width_usize;
                let cx = current % width_usize;
                min_x = min_x.min(cx);
                min_y = min_y.min(cy);
                max_x = max_x.max(cx);
                max_y = max_y.max(cy);
                sum_diff += diff_map[current] as u64;
                pixel_count += 1;

                for (dx, dy) in &[(0isize, 1), (1, 0), (0, -1), (-1, 0)] {
                    let nx = cx as isize + dx;
                    let ny = cy as isize + dy;
                    if nx < 0 || ny < 0 {
                        continue;
                    }
                    let nx = nx as usize;
                    let ny = ny as usize;
                    if nx >= width_usize || ny >= height_usize {
                        continue;
                    }
                    let neighbor = ny * width_usize + nx;
                    if visited[neighbor] || diff_map[neighbor] < PIXEL_DIFF_THRESHOLD {
                        continue;
                    }
                    visited[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }

            let score = if pixel_count == 0 {
                0.0
            } else {
                let max_total = (pixel_count as f64) * MAX_PIXEL_DIFF;
                (sum_diff as f64 / max_total).clamp(0.0, 1.0)
            };

            regions.push(ChangedRegion {
                bbox: [
                    min_x as u32,
                    min_y as u32,
                    (max_x + 1) as u32,
                    (max_y + 1) as u32,
                ],
                score,
                pixels: pixel_count,
            });
        }
    }

    regions
}

#[derive(Serialize)]
struct OcrEntry {
    step_index: usize,
    text: String,
    language: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::png::PngEncoder;
    use image::{ColorType, ImageEncoder, Rgba, RgbaImage};
    use predicates::prelude::{Predicate, predicate};
    use serde_json::Value;
    use std::error::Error;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    #[test]
    fn comparison_score_is_deterministic() {
        let baseline = RunArtifacts {
            run_id: "base".to_string(),
            steps: vec![StepScreenshot {
                step_index: 1,
                path: PathBuf::from("/tmp/base"),
                bytes: vec![0, 1, 2],
                sha256: sha256_hex(&[0, 1, 2]),
            }],
        };
        let candidate = RunArtifacts {
            run_id: "cand".to_string(),
            steps: vec![StepScreenshot {
                step_index: 1,
                path: PathBuf::from("/tmp/cand"),
                bytes: vec![0, 1],
                sha256: sha256_hex(&[0, 1]),
            }],
        };
        let comparisons = ComparisonSummary::between(&baseline, &candidate);
        assert_eq!(comparisons.len(), 1);
        assert!(predicate::eq(0.3333333333333333).eval(&comparisons[0].score));
    }

    #[test]
    fn visual_cli_generates_report() -> Result<(), Box<dyn Error>> {
        let temp = tempdir()?;
        let baseline = temp.path().join(".ralph").join("runs").join("baseline-run");
        fs::create_dir_all(&baseline)?;
        populate_run_artifacts(&baseline, &[b"alpha", b"beta"])?;

        let candidate = temp
            .path()
            .join(".ralph")
            .join("runs")
            .join("candidate-run");
        fs::create_dir_all(&candidate)?;
        populate_run_artifacts(&candidate, &[b"alpha"])?;

        let report_path = temp.path().join("visual-report.json");
        let args = VisualArgs {
            baseline: baseline.clone(),
            candidate: candidate.clone(),
            report: report_path.clone(),
        };
        run_command(args)?;

        let contents = fs::read_to_string(&report_path)?;
        let report: Value = serde_json::from_str(&contents)?;
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["baseline"]["run_id"], "baseline-run");
        assert_eq!(report["candidate"]["run_id"], "candidate-run");
        let comparisons = report["comparisons"].as_array().unwrap();
        assert_eq!(comparisons.len(), 1);
        Ok(())
    }

    fn populate_run_artifacts(root: &Path, snapshots: &[&[u8]]) -> std::io::Result<()> {
        for (index, bytes) in snapshots.iter().enumerate() {
            let step_dir = root.join("steps").join(format!("step-{}", index + 1));
            fs::create_dir_all(&step_dir)?;
            fs::write(step_dir.join("screenshot.png"), bytes)?;
        }
        Ok(())
    }

    #[test]
    fn changed_regions_empty_for_identical_frames() {
        let image = make_solid_image(4, 4, Rgba([0, 0, 0, 255]));
        let encoded = encode_png(&image);
        let regions = changed_regions_from_bytes(&encoded, &encoded);
        assert!(regions.is_empty());
    }

    #[test]
    fn changed_regions_reports_pixel_bbox() {
        let base = make_solid_image(4, 4, Rgba([0, 0, 0, 255]));
        let mut candidate = base.clone();
        candidate.put_pixel(1, 2, Rgba([255, 0, 0, 255]));

        let base_bytes = encode_png(&base);
        let candidate_bytes = encode_png(&candidate);

        let regions = changed_regions_from_bytes(&base_bytes, &candidate_bytes);
        assert_eq!(regions.len(), 1);

        let region = &regions[0];
        assert_eq!(region.bbox, [1, 2, 2, 3]);
        assert_eq!(region.pixels, 1);
        assert!(region.score > 0.0);
    }

    fn make_solid_image(width: u32, height: u32, color: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(width, height, color)
    }

    fn encode_png(image: &RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        let encoder = PngEncoder::new(&mut bytes);
        encoder
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                ColorType::Rgba8.into(),
            )
            .expect("failed to encode png");
        bytes
    }
}

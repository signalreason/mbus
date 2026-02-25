use crate::output::sha256_hex;
use clap::Args;
use image::{GenericImageView, RgbaImage, load_from_memory};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::Builder;
use time::OffsetDateTime;
use tracing::warn;

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

    /// Enable OCR extraction for changed regions (requires tesseract binary).
    #[arg(long)]
    pub enable_ocr: bool,

    /// Language hint for OCR (passed to tesseract via `-l`).
    #[arg(long, default_value = "eng")]
    pub ocr_language: String,
}

#[derive(Clone)]
struct OcrSettings {
    enabled: bool,
    language: String,
}

impl OcrSettings {
    fn from_args(args: &VisualArgs) -> Self {
        Self {
            enabled: args.enable_ocr,
            language: args.ocr_language.clone(),
        }
    }
}

/// Entrypoint for the visual evaluator utility.
pub fn run_command(args: VisualArgs) -> Result<(), Box<dyn Error>> {
    let ocr_settings = OcrSettings::from_args(&args);
    if ocr_settings.enabled {
        run_command_with_engine(&args, &ocr_settings, &TesseractOcr)
    } else {
        run_command_with_engine(&args, &ocr_settings, &NoopOcr)
    }
}

fn run_command_with_engine(
    args: &VisualArgs,
    ocr_settings: &OcrSettings,
    ocr_engine: &dyn OcrEngine,
) -> Result<(), Box<dyn Error>> {
    let baseline = RunArtifacts::load(&args.baseline)?;
    let candidate = RunArtifacts::load(&args.candidate)?;
    let report = VisualReport::from_runs(&baseline, &candidate, ocr_settings, ocr_engine);
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ocr_entries: Vec<OcrEntry>,
}

impl VisualReport {
    fn from_runs(
        baseline: &RunArtifacts,
        candidate: &RunArtifacts,
        ocr_settings: &OcrSettings,
        ocr_engine: &dyn OcrEngine,
    ) -> Self {
        let comparisons = ComparisonSummary::between(baseline, candidate, ocr_settings, ocr_engine);
        let mut ocr_entries = Vec::new();
        for comparison in &comparisons {
            for region in &comparison.changed_regions {
                if let Some(snippet) = &region.ocr {
                    ocr_entries.push(OcrEntry {
                        step_index: comparison.step_index,
                        bbox: region.bbox,
                        text: snippet.text.clone(),
                        language: snippet.language.clone(),
                    });
                }
            }
        }

        Self {
            schema_version: VISUAL_REPORT_SCHEMA_VERSION,
            generated_at: OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
            baseline: RunSummary::from_artifacts(baseline),
            candidate: RunSummary::from_artifacts(candidate),
            comparisons,
            ocr_entries,
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
    fn between(
        baseline: &RunArtifacts,
        candidate: &RunArtifacts,
        ocr_settings: &OcrSettings,
        ocr_engine: &dyn OcrEngine,
    ) -> Vec<Self> {
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
                let base_img = match load_rgba_image(&base.bytes) {
                    Some(img) => img,
                    None => continue,
                };
                let candidate_img = match load_rgba_image(&cand.bytes) {
                    Some(img) => img,
                    None => continue,
                };
                if base_img.dimensions() != candidate_img.dimensions() {
                    continue;
                }
                let mut changed_regions = changed_regions_from_images(&base_img, &candidate_img);
                if ocr_settings.enabled {
                    attach_ocr_snippets(
                        &mut changed_regions,
                        &candidate_img,
                        ocr_settings,
                        ocr_engine,
                    );
                }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    ocr: Option<OcrSnippet>,
}

fn load_rgba_image(bytes: &[u8]) -> Option<RgbaImage> {
    load_from_memory(bytes).ok().map(|img| img.to_rgba8())
}

fn changed_regions_from_images(
    base_img: &RgbaImage,
    candidate_img: &RgbaImage,
) -> Vec<ChangedRegion> {
    const PIXEL_DIFF_THRESHOLD: u32 = 30;
    const MAX_PIXEL_DIFF: f64 = 255.0 * 3.0;

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
                ocr: None,
            });
        }
    }

    regions
}

fn attach_ocr_snippets(
    regions: &mut [ChangedRegion],
    image: &RgbaImage,
    ocr_settings: &OcrSettings,
    ocr_engine: &dyn OcrEngine,
) {
    if !ocr_settings.enabled {
        return;
    }

    for region in regions.iter_mut() {
        if let Some(snippet) = ocr_engine.extract(image, region.bbox, &ocr_settings.language) {
            region.ocr = Some(snippet);
        }
    }
}

#[derive(Serialize, Clone)]
struct OcrSnippet {
    text: String,
    language: String,
}

#[derive(Serialize)]
struct OcrEntry {
    step_index: usize,
    bbox: [u32; 4],
    text: String,
    language: String,
}

trait OcrEngine {
    fn extract(&self, image: &RgbaImage, bbox: [u32; 4], language: &str) -> Option<OcrSnippet>;
}

struct NoopOcr;

impl OcrEngine for NoopOcr {
    fn extract(&self, _image: &RgbaImage, _bbox: [u32; 4], _language: &str) -> Option<OcrSnippet> {
        None
    }
}

struct TesseractOcr;

impl OcrEngine for TesseractOcr {
    fn extract(&self, image: &RgbaImage, bbox: [u32; 4], language: &str) -> Option<OcrSnippet> {
        let width = image.width();
        let height = image.height();
        let min_x = bbox[0].min(width);
        let min_y = bbox[1].min(height);
        let max_x = bbox[2].min(width);
        let max_y = bbox[3].min(height);

        if max_x <= min_x || max_y <= min_y {
            return None;
        }

        let region_width = max_x - min_x;
        let region_height = max_y - min_y;
        if region_width == 0 || region_height == 0 {
            return None;
        }

        let region = image
            .view(min_x, min_y, region_width, region_height)
            .to_image();
        let input_file = match Builder::new().suffix(".png").tempfile() {
            Ok(file) => file,
            Err(err) => {
                warn!("Unable to create temp file for OCR: {err}");
                return None;
            }
        };

        if region.save(input_file.path()).is_err() {
            warn!("Failed to write cropped region for OCR");
            return None;
        }

        let output_base = input_file.path().with_extension("");
        let output_txt = output_base.clone().with_extension("txt");
        let status = Command::new("tesseract")
            .arg(input_file.path())
            .arg(&output_base)
            .arg("-l")
            .arg(language)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match status {
            Ok(status) if status.success() => {}
            Ok(status) => {
                warn!("Tesseract failed for region {:?} ({})", bbox, status);
                return None;
            }
            Err(err) => {
                if err.kind() == ErrorKind::NotFound {
                    warn!(
                        "`tesseract` binary not found; skipping OCR for region {:?}",
                        bbox
                    );
                } else {
                    warn!(error = %err, "Failed to run tesseract for region {:?}", bbox);
                }
                return None;
            }
        }

        let mut raw_text = String::new();
        if let Err(err) =
            fs::File::open(&output_txt).and_then(|mut file| file.read_to_string(&mut raw_text))
        {
            warn!("Unable to read OCR output for region {:?}: {err}", bbox);
            let _ = fs::remove_file(&output_txt);
            return None;
        }

        let _ = fs::remove_file(&output_txt);

        let trimmed = raw_text.trim();
        if trimmed.is_empty() {
            return None;
        }

        Some(OcrSnippet {
            text: trimmed.to_string(),
            language: language.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::png::PngEncoder;
    use image::{ColorType, ImageEncoder, Rgba, RgbaImage};
    use serde_json::Value;
    use std::error::Error;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    struct StubOcr;

    impl OcrEngine for StubOcr {
        fn extract(&self, _: &RgbaImage, _: [u32; 4], language: &str) -> Option<OcrSnippet> {
            Some(OcrSnippet {
                text: "stubbed text".to_string(),
                language: language.to_string(),
            })
        }
    }

    #[test]
    fn comparison_score_is_deterministic() {
        let baseline_bytes = encode_png(&make_solid_image(2, 2, Rgba([0, 0, 0, 255])));
        let candidate_bytes = encode_png(&make_solid_image(2, 2, Rgba([255, 0, 0, 255])));
        let baseline = RunArtifacts {
            run_id: "base".to_string(),
            steps: vec![StepScreenshot {
                step_index: 1,
                path: PathBuf::from("/tmp/base"),
                bytes: baseline_bytes.clone(),
                sha256: sha256_hex(&baseline_bytes),
            }],
        };
        let candidate = RunArtifacts {
            run_id: "cand".to_string(),
            steps: vec![StepScreenshot {
                step_index: 1,
                path: PathBuf::from("/tmp/cand"),
                bytes: candidate_bytes.clone(),
                sha256: sha256_hex(&candidate_bytes),
            }],
        };
        let ocr_settings = OcrSettings {
            enabled: false,
            language: "eng".to_string(),
        };
        let engine = NoopOcr;

        let comparisons = ComparisonSummary::between(&baseline, &candidate, &ocr_settings, &engine);
        let replay = ComparisonSummary::between(&baseline, &candidate, &ocr_settings, &engine);
        assert_eq!(comparisons.len(), 1);
        assert_eq!(comparisons[0].score, replay[0].score);
        assert!(comparisons[0].score.is_finite());
    }

    #[test]
    fn visual_cli_generates_report() -> Result<(), Box<dyn Error>> {
        let temp = tempdir()?;
        let baseline = temp.path().join(".ralph").join("runs").join("baseline-run");
        fs::create_dir_all(&baseline)?;
        let baseline_frames = vec![
            encode_png(&make_solid_image(2, 2, Rgba([0, 0, 0, 255]))),
            encode_png(&make_solid_image(2, 2, Rgba([0, 0, 255, 255]))),
        ];
        populate_run_artifacts(&baseline, &baseline_frames)?;

        let candidate = temp
            .path()
            .join(".ralph")
            .join("runs")
            .join("candidate-run");
        fs::create_dir_all(&candidate)?;
        let candidate_frames = vec![encode_png(&make_solid_image(2, 2, Rgba([255, 0, 0, 255])))];
        populate_run_artifacts(&candidate, &candidate_frames)?;

        let report_path = temp.path().join("visual-report.json");
        let args = VisualArgs {
            baseline: baseline.clone(),
            candidate: candidate.clone(),
            report: report_path.clone(),
            enable_ocr: false,
            ocr_language: "eng".to_string(),
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

    #[test]
    fn visual_cli_generates_report_with_ocr_snippets() -> Result<(), Box<dyn Error>> {
        let temp = tempdir()?;
        let baseline = temp.path().join(".ralph").join("runs").join("baseline-run");
        fs::create_dir_all(&baseline)?;
        let baseline_frames = vec![
            encode_png(&make_solid_image(2, 2, Rgba([0, 0, 0, 255]))),
            encode_png(&make_solid_image(2, 2, Rgba([0, 0, 255, 255]))),
        ];
        populate_run_artifacts(&baseline, &baseline_frames)?;

        let candidate = temp
            .path()
            .join(".ralph")
            .join("runs")
            .join("candidate-run");
        fs::create_dir_all(&candidate)?;
        let candidate_frames = vec![encode_png(&make_solid_image(2, 2, Rgba([255, 0, 0, 255])))];
        populate_run_artifacts(&candidate, &candidate_frames)?;

        let report_path = temp.path().join("visual-report.json");
        let args = VisualArgs {
            baseline: baseline.clone(),
            candidate: candidate.clone(),
            report: report_path.clone(),
            enable_ocr: true,
            ocr_language: "stub-lang".to_string(),
        };
        let ocr_settings = OcrSettings::from_args(&args);
        super::run_command_with_engine(&args, &ocr_settings, &StubOcr)?;

        let contents = fs::read_to_string(&report_path)?;
        let report: Value = serde_json::from_str(&contents)?;
        let ocr_entries = report["ocr_entries"]
            .as_array()
            .expect("expected ocr entries");
        assert!(!ocr_entries.is_empty());

        let comparisons = report["comparisons"]
            .as_array()
            .expect("expected comparisons array");
        let changed_regions = comparisons[0]["changed_regions"]
            .as_array()
            .expect("expected changed regions");
        let snippet = changed_regions[0]["ocr"]
            .as_object()
            .expect("expected ocr snippet");
        assert_eq!(snippet["text"].as_str().unwrap(), "stubbed text");
        assert_eq!(snippet["language"].as_str().unwrap(), "stub-lang");
        Ok(())
    }

    #[test]
    fn ocr_enabled_report_attaches_snippets() {
        let baseline_bytes = encode_png(&make_solid_image(3, 3, Rgba([0, 0, 0, 255])));
        let candidate_bytes = encode_png(&make_solid_image(3, 3, Rgba([0, 255, 0, 255])));
        let baseline = RunArtifacts {
            run_id: "base".to_string(),
            steps: vec![StepScreenshot {
                step_index: 1,
                path: PathBuf::from("/tmp/base"),
                bytes: baseline_bytes.clone(),
                sha256: sha256_hex(&baseline_bytes),
            }],
        };
        let candidate = RunArtifacts {
            run_id: "cand".to_string(),
            steps: vec![StepScreenshot {
                step_index: 1,
                path: PathBuf::from("/tmp/cand"),
                bytes: candidate_bytes.clone(),
                sha256: sha256_hex(&candidate_bytes),
            }],
        };
        let ocr_settings = OcrSettings {
            enabled: true,
            language: "stub-lang".to_string(),
        };
        let report = VisualReport::from_runs(&baseline, &candidate, &ocr_settings, &StubOcr);

        assert!(!report.ocr_entries.is_empty());
        let comparison = &report.comparisons[0];
        let region = comparison
            .changed_regions
            .first()
            .expect("expected changed region");
        let snippet = region.ocr.as_ref().expect("expected ocr snippet");
        assert_eq!(snippet.text, "stubbed text");
        assert_eq!(snippet.language, "stub-lang");
        assert_eq!(report.ocr_entries[0].text, snippet.text);
        assert_eq!(report.ocr_entries[0].language, snippet.language);
        assert_eq!(report.ocr_entries[0].bbox, region.bbox);
    }

    fn populate_run_artifacts(root: &Path, snapshots: &[Vec<u8>]) -> std::io::Result<()> {
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
        let regions = changed_regions_from_images(&image, &image);
        assert!(regions.is_empty());
    }

    #[test]
    fn changed_regions_reports_pixel_bbox() {
        let base = make_solid_image(4, 4, Rgba([0, 0, 0, 255]));
        let mut candidate = base.clone();
        candidate.put_pixel(1, 2, Rgba([255, 0, 0, 255]));

        let regions = changed_regions_from_images(&base, &candidate);
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

use clap::{Args, Parser, Subcommand};
use mbus::agent::r#loop::{AgentLoop, LlmClients, RunStatus};
use mbus::bench::aggregate::{
    aggregate_usage_from_results, aggregate_usage_from_steps, estimate_cost,
};
use mbus::bench::{
    BENCH_REPORT_SCHEMA_VERSION, BenchLlmInfo, BenchObservedStatus, BenchPricing, BenchReport,
    BenchServer, BenchTaskResult, BenchTokenUsage, actions_file_path, actions_work_dir,
    bench_task_limit, build_summary, evaluate_gate, evaluate_task, failure_buckets, join_base_url,
    load_tasks, now_timestamp, render_actions, report_path_default, sleep_between_tasks,
    tasks_dir_default, write_actions_file, write_report,
};
use mbus::browser::CdpBrowser;
use mbus::config::{CliOverrides, ConfigError, LlmConfig, LlmMode, ScreenshotPersist, load_config};
use mbus::llm::openai::{OpenAiClient, OpenAiConfig};
use mbus::llm::router::Router;
use mbus::llm::scripted::{ScriptedLlm, StubLlm};
use mbus::telemetry;
use mbus::types::ReasoningEffort;
use mbus::verify::rules::Validator;
use mbus::visual::{self, VisualArgs};
use serde::Serialize;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Parser, Debug)]
#[command(name = "mbus", version, about = "Rust browser + LLM agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
enum Commands {
    Run(RunArgs),
    Bench(BenchArgs),
    Visual(VisualArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    #[arg(
        long,
        required_unless_present = "task_file",
        conflicts_with = "task_file"
    )]
    task: Option<String>,
    #[arg(long, required_unless_present = "task", conflicts_with = "task")]
    task_file: Option<PathBuf>,
    #[arg(long, conflicts_with = "plan_file")]
    plan: Option<String>,
    #[arg(long, conflicts_with = "plan")]
    plan_file: Option<PathBuf>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    headless: Option<bool>,
    #[arg(long)]
    initial_url: Option<String>,
    #[arg(long)]
    cdp_url: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(usize))]
    max_steps: Option<usize>,
    #[arg(long, value_parser = clap::value_parser!(usize))]
    max_no_progress_steps: Option<usize>,
    #[arg(long)]
    memory_max_observations: Option<usize>,
    #[arg(long)]
    memory_max_history: Option<usize>,
    #[arg(long)]
    snapshot_timeout_ms: Option<u64>,
    #[arg(long)]
    action_timeout_ms: Option<u64>,
    #[arg(long)]
    max_elements: Option<usize>,
    #[arg(long)]
    max_text_len: Option<usize>,
    #[arg(long)]
    router_failures_to_mid: Option<u32>,
    #[arg(long)]
    router_failures_to_strong: Option<u32>,
    #[arg(long)]
    router_no_progress_to_mid: Option<u32>,
    #[arg(long)]
    router_no_progress_to_strong: Option<u32>,
    #[arg(long)]
    router_reasoning_effort: Option<String>,
    #[arg(long)]
    router_ladder: Vec<String>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    allow_insecure: Option<bool>,
    #[arg(long)]
    validator_max_text_len: Option<usize>,
    #[arg(long)]
    validator_max_wait_ms: Option<u64>,
    #[arg(long)]
    validator_max_scroll: Option<i64>,
    #[arg(long)]
    llm_mode: Option<String>,
    #[arg(long)]
    llm_base_url: Option<String>,
    #[arg(long)]
    llm_api_key: Option<String>,
    #[arg(long)]
    llm_model_fast: Option<String>,
    #[arg(long)]
    llm_model_mid: Option<String>,
    #[arg(long)]
    llm_model_strong: Option<String>,
    #[arg(long)]
    llm_timeout_ms: Option<u64>,
    #[arg(long)]
    llm_temperature: Option<f32>,
    #[arg(long)]
    llm_max_tokens: Option<u32>,
    #[arg(long)]
    llm_actions_file: Option<PathBuf>,
    #[arg(long)]
    extract_output: Option<PathBuf>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    screenshot_enabled: Option<bool>,
    #[arg(long)]
    screenshot_persist: Option<String>,
}

#[derive(Args, Debug)]
struct BenchArgs {
    #[arg(long)]
    tasks_dir: Option<PathBuf>,
    #[arg(long)]
    report_path: Option<PathBuf>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    headless: Option<bool>,
    #[arg(long)]
    max_steps_per_task: Option<usize>,
    #[arg(long)]
    required_passes: Option<usize>,
    #[arg(long)]
    router_ladder: Vec<String>,
    #[arg(long)]
    llm_mode: Option<String>,
    #[arg(long)]
    llm_base_url: Option<String>,
    #[arg(long)]
    llm_api_key: Option<String>,
    #[arg(long)]
    llm_model_fast: Option<String>,
    #[arg(long)]
    llm_model_mid: Option<String>,
    #[arg(long)]
    llm_model_strong: Option<String>,
    #[arg(long)]
    llm_timeout_ms: Option<u64>,
    #[arg(long)]
    llm_temperature: Option<f32>,
    #[arg(long)]
    llm_max_tokens: Option<u32>,
    #[arg(long)]
    llm_input_cost_per_million: Option<f64>,
    #[arg(long)]
    llm_output_cost_per_million: Option<f64>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    screenshot_enabled: Option<bool>,
    #[arg(long)]
    screenshot_persist: Option<String>,
}

#[tokio::main]
async fn main() {
    telemetry::init_tracing();
    if let Err(err) = run_cli().await {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

async fn run_cli() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run(args) => run_command(args).await,
        Commands::Bench(args) => bench_command(args).await,
        Commands::Visual(args) => visual::run_command(args),
    }
}

async fn run_command(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let task = resolve_required_text("task", args.task.as_deref(), args.task_file.as_deref())?;
    let plan = resolve_optional_text("plan", args.plan.as_deref(), args.plan_file.as_deref())?;

    let config_path = resolve_config_path(args.config.as_deref());
    let cli_overrides = build_cli_overrides(&args)?;
    let config = load_config(config_path.as_deref(), cli_overrides)?;
    let task_id = mbus::output::task_id_for(&task);
    let run_timestamp = mbus::output::current_timestamp()?;
    let run_id = mbus::output::run_id_for(&task_id, &run_timestamp);

    emit_json(&ConfigLog::from(&config))?;
    let repair_start = telemetry::snapshot();
    let execution = match execute_agent(&task, plan.as_deref(), &config).await {
        Ok(execution) => execution,
        Err(err) => {
            let end_snapshot = telemetry::snapshot();
            let repair_counts = repair_counts_delta(&repair_start, &end_snapshot);
            let screenshot_counts = screenshot_counts_delta(&repair_start, &end_snapshot);
            let router_state = router_final_state(&Router::new(config.router.clone()));
            let summary = mbus::output::build_run_summary(
                mbus::output::TerminalState::Error,
                &[],
                vec![run_error_summary(
                    "startup_error",
                    err.to_string(),
                    Some("startup"),
                )],
                Vec::new(),
                repair_counts,
                screenshot_counts,
                router_state,
            );
            emit_run_logs(&[], &summary, None, None)?;
            return Err(err);
        }
    };

    let RunExecution {
        result,
        steps,
        final_observation,
        step_screenshots,
        router_final_state,
    } = execution;

    let mut errors = Vec::new();
    let mut output_artifacts = Vec::new();
    let mut final_action = None;
    let mut terminal_state = mbus::output::TerminalState::Error;
    let mut return_error: Option<Box<dyn Error>> = None;

    match result {
        Ok(result) => {
            terminal_state = match result.status {
                RunStatus::Done => mbus::output::TerminalState::Done,
                RunStatus::MaxSteps => mbus::output::TerminalState::MaxSteps,
                RunStatus::NoProgress => mbus::output::TerminalState::NoProgress,
            };
            final_action = Some(result.final_action);
        }
        Err(err) => {
            errors.push(agent_error_summary(&err));
            if return_error.is_none() {
                return_error = Some(Box::new(err));
            }
        }
    }

    match write_extract_output(&task, &config, &steps, &task_id, &run_timestamp) {
        Ok(Some(artifact)) => output_artifacts.push(artifact),
        Ok(None) => {}
        Err(err) => {
            errors.push(run_error_summary(
                "output_error",
                err.to_string(),
                Some("output"),
            ));
            if return_error.is_none() {
                return_error = Some(err);
            }
        }
    }

    match write_transition_trace(&run_id, &task, &steps, &task_id, &run_timestamp) {
        Ok(Some(artifact)) => output_artifacts.push(artifact),
        Ok(None) => {}
        Err(err) => {
            errors.push(run_error_summary(
                "output_error",
                err.to_string(),
                Some("output"),
            ));
            if return_error.is_none() {
                return_error = Some(err);
            }
        }
    }

    let screenshot_result =
        write_screenshot_artifacts(&config, &run_id, &terminal_state, &step_screenshots);
    output_artifacts.extend(screenshot_result.artifacts);
    errors.extend(screenshot_result.errors);

    let end_snapshot = telemetry::snapshot();
    let repair_counts = repair_counts_delta(&repair_start, &end_snapshot);
    let screenshot_counts = screenshot_counts_delta(&repair_start, &end_snapshot);
    let summary = mbus::output::build_run_summary(
        terminal_state,
        &steps,
        errors,
        output_artifacts,
        repair_counts,
        screenshot_counts,
        router_final_state,
    );
    emit_run_logs(
        &steps,
        &summary,
        final_action.as_ref(),
        final_observation.as_ref(),
    )?;

    if let Some(err) = return_error {
        return Err(err);
    }

    Ok(())
}

async fn bench_command(args: BenchArgs) -> Result<(), Box<dyn Error>> {
    let tasks_dir = args.tasks_dir.clone().unwrap_or_else(tasks_dir_default);
    let report_path = args.report_path.clone().unwrap_or_else(report_path_default);
    let max_steps_per_task = args.max_steps_per_task.unwrap_or(40);

    let tasks = load_tasks(&tasks_dir).map_err(|err| format!("bench tasks: {err}"))?;
    let total_tasks = tasks.len();
    let required_passes = args
        .required_passes
        .unwrap_or_else(|| total_tasks.saturating_sub(2));

    let config_path = resolve_config_path(args.config.as_deref());
    let cli_overrides = build_bench_cli_overrides(&args, max_steps_per_task)?;
    let base_config = load_config(config_path.as_deref(), cli_overrides)?;
    let bench_llm_mode = match base_config.llm.mode {
        LlmMode::Scripted => LlmMode::Scripted,
        LlmMode::OpenAi => LlmMode::OpenAi,
        LlmMode::Stub => {
            return Err(
                "bench requires llm.mode scripted or openai (set --llm-mode or config [llm].mode)"
                    .into(),
            );
        }
    };

    let server = BenchServer::start()
        .await
        .map_err(|err| format!("bench server startup failed: {err}"))?;
    let base_url = server.base_url();
    let llm_info = BenchLlmInfo {
        mode: llm_mode_label(&bench_llm_mode).to_string(),
        model_fast: base_config.llm.model_fast.clone(),
        model_mid: base_config.llm.model_mid.clone(),
        model_strong: base_config.llm.model_strong.clone(),
    };

    emit_json(&BenchConfigLog {
        r#type: "bench_config",
        tasks_dir: tasks_dir.display().to_string(),
        report_path: report_path.display().to_string(),
        max_steps_per_task,
        required_passes,
        base_url: base_url.clone(),
    })?;

    let actions_dir = actions_work_dir(&report_path);
    let bench_started_at = std::time::Instant::now();
    let mut results = Vec::with_capacity(tasks.len());

    for task in tasks {
        let started_at = std::time::Instant::now();
        let mut task_config = base_config.clone();
        let step_limit = bench_task_limit(&task, max_steps_per_task);
        task_config.agent.max_steps = step_limit;
        task_config.browser.initial_url = join_base_url(&base_url, &task.start_path);
        task_config.llm.mode = bench_llm_mode.clone();

        if bench_llm_mode == LlmMode::Scripted {
            let actions_json = render_actions(&task.actions, &base_url)
                .map_err(|err| format!("task {} actions: {err}", task.id))?;
            let actions_file = actions_file_path(&actions_dir, &task.id);
            write_actions_file(&actions_file, &actions_json)
                .map_err(|err| format!("task {} actions file: {err}", task.id))?;
            task_config.llm.actions_file = Some(actions_file);
        } else {
            task_config.llm.actions_file = None;
        }

        let run = execute_agent(&task.task, task.plan.as_deref(), &task_config).await;
        let usage = match &run {
            Ok(execution) => aggregate_usage_from_steps(&execution.steps, &bench_llm_mode),
            Err(_) => aggregate_usage_from_steps(&[], &bench_llm_mode),
        };
        let elapsed = started_at.elapsed().as_millis() as u64;
        let task_result = match run {
            Ok(execution) => match execution.result {
                Ok(result) => {
                    let observed_status = match result.status {
                        RunStatus::Done => BenchObservedStatus::Done,
                        RunStatus::MaxSteps => BenchObservedStatus::MaxSteps,
                        RunStatus::NoProgress => BenchObservedStatus::NoProgress,
                    };
                    let mut evaluated = evaluate_task(
                        &task,
                        observed_status,
                        result.steps.len(),
                        Some(&result.final_observation.url),
                        Some(&result.final_observation.visible_text),
                        step_limit,
                        None,
                        usage,
                    );
                    evaluated.duration_ms = elapsed;
                    evaluated
                }
                Err(err) => {
                    let mut evaluated = evaluate_task(
                        &task,
                        BenchObservedStatus::Error,
                        execution.steps.len(),
                        None,
                        None,
                        step_limit,
                        Some(&err.to_string()),
                        usage,
                    );
                    evaluated.duration_ms = elapsed;
                    evaluated
                }
            },
            Err(err) => {
                let mut evaluated = evaluate_task(
                    &task,
                    BenchObservedStatus::Error,
                    0,
                    None,
                    None,
                    step_limit,
                    Some(&err.to_string()),
                    usage,
                );
                evaluated.duration_ms = elapsed;
                evaluated
            }
        };
        emit_json(&BenchTaskLog::from(&task_result))?;
        results.push(task_result);
        sleep(sleep_between_tasks()).await;
    }

    let gate = evaluate_gate(&results, required_passes);
    let summary = build_summary(&results, &gate);
    let aggregate_usage = aggregate_usage_from_results(&results);
    let aggregate_cost = estimate_cost(
        &aggregate_usage,
        BenchPricing::from_config(&base_config.llm),
    );
    let bench_duration_ms = bench_started_at.elapsed().as_millis() as u64;
    let report = BenchReport {
        schema_version: BENCH_REPORT_SCHEMA_VERSION,
        timestamp: now_timestamp().map_err(|err| format!("bench timestamp: {err}"))?,
        tasks_dir: tasks_dir.display().to_string(),
        report_path: report_path.display().to_string(),
        llm: llm_info,
        max_steps_per_task,
        required_passes,
        duration_ms: bench_duration_ms,
        gate: gate.clone(),
        summary: summary.clone(),
        aggregate_usage,
        aggregate_cost,
        results,
    };
    write_report(&report_path, &report)
        .map_err(|err| format!("failed to write bench report: {err}"))?;

    emit_json(&BenchSummaryLog {
        r#type: "bench_summary",
        total_tasks: summary.total_tasks,
        passed_tasks: summary.passed_tasks,
        required_passes: summary.required_passes,
        completion_rate: summary.completion_rate,
        median_steps_success: summary.median_steps_success,
        p95_steps_success: summary.p95_steps_success,
        gate_passed: summary.gate_passed,
        failure_buckets: failure_buckets(&report.results),
        report_path: report_path.display().to_string(),
    })?;

    server.shutdown().await;

    if !gate.passed {
        let reason = gate
            .reason
            .unwrap_or_else(|| "benchmark gate failed".to_string());
        return Err(reason.into());
    }

    Ok(())
}

fn llm_mode_label(mode: &LlmMode) -> &'static str {
    match mode {
        LlmMode::Scripted => "scripted",
        LlmMode::OpenAi => "openai",
        LlmMode::Stub => "stub",
    }
}

fn resolve_config_path(cli_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = cli_path {
        return Some(path.to_path_buf());
    }
    if let Ok(path) = std::env::var("MBUS_CONFIG") {
        return Some(PathBuf::from(path));
    }
    if let Ok(cwd) = std::env::current_dir() {
        let local = cwd.join("mbus.toml");
        if local.is_file() {
            return Some(local);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let home_config = PathBuf::from(home).join(".mbus.toml");
        if home_config.is_file() {
            return Some(home_config);
        }
    }
    None
}

fn resolve_required_text(
    label: &str,
    inline: Option<&str>,
    file_path: Option<&Path>,
) -> Result<String, Box<dyn Error>> {
    match (inline, file_path) {
        (Some(_), Some(_)) => {
            Err(format!("{label}: use only one of --{label} or --{label}-file").into())
        }
        (Some(value), None) => Ok(value.to_string()),
        (None, Some(path)) => {
            let content = std::fs::read_to_string(path)?;
            Ok(content.trim().to_string())
        }
        (None, None) => Err(format!("{label} is required").into()),
    }
}

fn resolve_optional_text(
    label: &str,
    inline: Option<&str>,
    file_path: Option<&Path>,
) -> Result<Option<String>, Box<dyn Error>> {
    match (inline, file_path) {
        (Some(_), Some(_)) => {
            Err(format!("{label}: use only one of --{label} or --{label}-file").into())
        }
        (Some(value), None) => Ok(Some(value.to_string())),
        (None, Some(path)) => {
            let content = std::fs::read_to_string(path)?;
            Ok(Some(content.trim().to_string()))
        }
        (None, None) => Ok(None),
    }
}

fn parse_reasoning_effort(value: &str) -> Result<ReasoningEffort, ConfigError> {
    ReasoningEffort::from_str(value).map_err(|message| ConfigError::Invalid { message })
}

fn build_cli_overrides(args: &RunArgs) -> Result<CliOverrides, ConfigError> {
    let llm_mode = if let Some(mode) = args.llm_mode.as_deref() {
        Some(LlmMode::from_str(mode)?)
    } else if args.llm_actions_file.is_some() {
        Some(LlmMode::Scripted)
    } else {
        None
    };
    let screenshot_persist = if let Some(value) = args.screenshot_persist.as_deref() {
        Some(ScreenshotPersist::from_str(value)?)
    } else {
        None
    };
    let router_reasoning_effort = if let Some(value) = args.router_reasoning_effort.as_deref() {
        Some(parse_reasoning_effort(value)?)
    } else {
        None
    };
    let router_ladder = if args.router_ladder.is_empty() {
        None
    } else {
        Some(args.router_ladder.clone())
    };

    Ok(CliOverrides {
        max_steps: args.max_steps,
        max_no_progress_steps: args.max_no_progress_steps,
        memory_max_observations: args.memory_max_observations,
        memory_max_history: args.memory_max_history,
        headless: args.headless,
        initial_url: args.initial_url.clone(),
        cdp_url: args.cdp_url.clone(),
        snapshot_timeout_ms: args.snapshot_timeout_ms,
        action_timeout_ms: args.action_timeout_ms,
        max_elements: args.max_elements,
        max_text_len: args.max_text_len,
        router_failures_to_mid: args.router_failures_to_mid,
        router_failures_to_strong: args.router_failures_to_strong,
        router_no_progress_to_mid: args.router_no_progress_to_mid,
        router_no_progress_to_strong: args.router_no_progress_to_strong,
        router_reasoning_effort,
        router_ladder,
        allow_insecure: args.allow_insecure,
        validator_max_text_len: args.validator_max_text_len,
        validator_max_wait_ms: args.validator_max_wait_ms,
        validator_max_scroll: args.validator_max_scroll,
        llm_mode,
        llm_base_url: args.llm_base_url.clone(),
        llm_api_key: args.llm_api_key.clone(),
        llm_model_fast: args.llm_model_fast.clone(),
        llm_model_mid: args.llm_model_mid.clone(),
        llm_model_strong: args.llm_model_strong.clone(),
        llm_timeout_ms: args.llm_timeout_ms,
        llm_temperature: args.llm_temperature,
        llm_max_tokens: args.llm_max_tokens,
        llm_input_cost_per_million: None,
        llm_output_cost_per_million: None,
        llm_actions_file: args.llm_actions_file.clone(),
        extract_output: args.extract_output.clone(),
        screenshot_enabled: args.screenshot_enabled,
        screenshot_persist,
    })
}

fn build_bench_cli_overrides(
    args: &BenchArgs,
    max_steps_per_task: usize,
) -> Result<CliOverrides, ConfigError> {
    let llm_mode = if let Some(mode) = args.llm_mode.as_deref() {
        Some(LlmMode::from_str(mode)?)
    } else {
        None
    };
    let screenshot_persist = if let Some(value) = args.screenshot_persist.as_deref() {
        Some(ScreenshotPersist::from_str(value)?)
    } else {
        None
    };
    let router_ladder = if args.router_ladder.is_empty() {
        None
    } else {
        Some(args.router_ladder.clone())
    };

    Ok(CliOverrides {
        max_steps: Some(max_steps_per_task),
        headless: args.headless,
        router_ladder,
        llm_mode,
        llm_base_url: args.llm_base_url.clone(),
        llm_api_key: args.llm_api_key.clone(),
        llm_model_fast: args.llm_model_fast.clone(),
        llm_model_mid: args.llm_model_mid.clone(),
        llm_model_strong: args.llm_model_strong.clone(),
        llm_timeout_ms: args.llm_timeout_ms,
        llm_temperature: args.llm_temperature,
        llm_max_tokens: args.llm_max_tokens,
        llm_input_cost_per_million: args.llm_input_cost_per_million,
        llm_output_cost_per_million: args.llm_output_cost_per_million,
        screenshot_enabled: args.screenshot_enabled,
        screenshot_persist,
        ..CliOverrides::default()
    })
}

fn build_clients(config: &LlmConfig) -> Result<LlmClients, Box<dyn Error>> {
    match config.mode {
        LlmMode::Stub => {
            let client = StubLlm::new("stub done");
            Ok(LlmClients::new(
                Box::new(client.clone()),
                Box::new(client.clone()),
                Box::new(client),
            ))
        }
        LlmMode::Scripted => {
            let path = config
                .actions_file
                .as_ref()
                .ok_or("llm.actions_file is required for scripted mode")?;
            let client = ScriptedLlm::from_path(path)?;
            Ok(LlmClients::new(
                Box::new(client.clone()),
                Box::new(client.clone()),
                Box::new(client),
            ))
        }
        LlmMode::OpenAi => {
            let api_key = config
                .api_key
                .as_ref()
                .ok_or("llm.api_key is required for openai mode")?;
            let timeout = Duration::from_millis(config.timeout_ms);
            let fast = OpenAiClient::new(OpenAiConfig {
                api_key: api_key.to_string(),
                base_url: config.base_url.clone(),
                model: config.model_fast.clone(),
                timeout,
                temperature: config.temperature,
                max_tokens: config.max_tokens,
            })?;
            let mid = OpenAiClient::new(OpenAiConfig {
                api_key: api_key.to_string(),
                base_url: config.base_url.clone(),
                model: config.model_mid.clone(),
                timeout,
                temperature: config.temperature,
                max_tokens: config.max_tokens,
            })?;
            let strong = OpenAiClient::new(OpenAiConfig {
                api_key: api_key.to_string(),
                base_url: config.base_url.clone(),
                model: config.model_strong.clone(),
                timeout,
                temperature: config.temperature,
                max_tokens: config.max_tokens,
            })?;
            Ok(LlmClients::new(
                Box::new(fast),
                Box::new(mid),
                Box::new(strong),
            ))
        }
    }
}

struct RunExecution {
    result: Result<mbus::agent::r#loop::RunResult, mbus::agent::r#loop::AgentError>,
    steps: Vec<mbus::agent::memory::StepRecord>,
    final_observation: Option<mbus::types::Observation>,
    step_screenshots: Vec<Option<Vec<u8>>>,
    router_final_state: mbus::output::RouterFinalState,
}

#[derive(Default)]
struct ScreenshotPersistResult {
    artifacts: Vec<mbus::output::OutputArtifact>,
    errors: Vec<mbus::output::RunErrorSummary>,
}

fn run_error_summary(
    code: impl Into<String>,
    message: impl Into<String>,
    kind: Option<&str>,
) -> mbus::output::RunErrorSummary {
    mbus::output::RunErrorSummary {
        code: code.into(),
        message: message.into(),
        step_index: None,
        field: None,
        validation_code: None,
        kind: kind.map(|value| value.to_string()),
    }
}

fn agent_error_summary(err: &mbus::agent::r#loop::AgentError) -> mbus::output::RunErrorSummary {
    match err {
        mbus::agent::r#loop::AgentError::Browser(err) => {
            run_error_summary(err.code, err.message.clone(), Some("browser"))
        }
        mbus::agent::r#loop::AgentError::Llm(err) => {
            run_error_summary(err.code, err.message.clone(), Some("llm"))
        }
    }
}

fn router_final_state(router: &Router) -> mbus::output::RouterFinalState {
    let ladder_index = router.ladder_index();
    let step = router
        .ladder()
        .get(ladder_index)
        .or_else(|| router.ladder().last());
    let (model, effort, tier) = match step {
        Some(step) => (step.model.clone(), step.effort, step.tier),
        None => ("unknown".to_string(), router.effort(), router.active_tier()),
    };
    mbus::output::RouterFinalState {
        model,
        effort,
        tier: tier_label(tier).to_string(),
        ladder_index,
    }
}

fn tier_label(tier: mbus::llm::router::Tier) -> &'static str {
    match tier {
        mbus::llm::router::Tier::Fast => "fast",
        mbus::llm::router::Tier::Mid => "mid",
        mbus::llm::router::Tier::Strong => "strong",
    }
}

async fn execute_agent(
    task: &str,
    plan: Option<&str>,
    config: &mbus::config::AppConfig,
) -> Result<RunExecution, Box<dyn Error>> {
    let mut browser_config = config.browser.clone();
    browser_config.max_scroll = config.validator.max_scroll;
    browser_config.max_wait_ms = config.validator.max_wait_ms;
    browser_config.screenshot_enabled = config.screenshot.enabled;
    let browser = CdpBrowser::start(browser_config).await?;
    let clients = build_clients(&config.llm)?;

    let mut agent = AgentLoop::new(browser, clients, task.to_string())
        .with_policy(config.agent.clone())
        .with_router(Router::new(config.router.clone()))
        .with_validator(Validator::new(config.validator.clone()));
    if let Some(plan) = plan {
        agent = agent.with_plan(plan.to_string());
    }

    let run_result = agent.run().await;
    let step_screenshots = match &run_result {
        Ok(result) => result.step_screenshots.clone(),
        Err(_) => Vec::new(),
    };
    let steps = agent.memory().steps().to_vec();
    let final_observation = agent.memory().observations().back().cloned();
    let router_final_state = router_final_state(agent.router());
    let shutdown_result = agent.shutdown().await;
    if let Err(err) = shutdown_result {
        eprintln!("shutdown error: {err}");
    }

    Ok(RunExecution {
        result: run_result,
        steps,
        final_observation,
        step_screenshots,
        router_final_state,
    })
}

fn emit_run_logs(
    steps: &[mbus::agent::memory::StepRecord],
    summary: &mbus::output::RunSummary,
    final_action: Option<&mbus::types::Action>,
    final_observation: Option<&mbus::types::Observation>,
) -> Result<(), Box<dyn Error>> {
    for (index, step) in steps.iter().enumerate() {
        emit_json(&StepLog {
            r#type: "step",
            index: index + 1,
            action: step.action.clone(),
            validation: step.validation.clone(),
            result: step.result.clone(),
            outcome: step.outcome.clone(),
            timings: step.timings.clone(),
            llm_payload_mode: step.llm_payload_mode,
            image_context_sent: step.llm_payload_mode.image_context_sent(),
            llm_usage: step.llm_usage.clone(),
            router: step.router.clone(),
        })?;
    }

    emit_json(&SummaryLog {
        r#type: "summary",
        status: match summary.terminal_state {
            mbus::output::TerminalState::Done => "done",
            mbus::output::TerminalState::MaxSteps => "max_steps",
            mbus::output::TerminalState::NoProgress => "no_progress",
            mbus::output::TerminalState::Error => "error",
        },
        terminal_state: summary.terminal_state.clone(),
        final_action: final_action.cloned(),
        steps: summary.steps,
        validation_failures: summary.validation_failures,
        apply_failures: summary.apply_failures,
        apply_successes: summary.apply_successes,
        done_steps: summary.done_steps,
        repair_attempts: summary.repair_attempts,
        repair_successes: summary.repair_successes,
        repair_failures: summary.repair_failures,
        screenshots: summary.screenshots.clone(),
        router: summary.router.clone(),
        errors: summary.errors.clone(),
        output_artifacts: summary.output_artifacts.clone(),
        final_url: final_observation.map(|value| value.url.clone()),
        final_title: final_observation.map(|value| value.title.clone()),
    })?;

    Ok(())
}

fn write_extract_output(
    task: &str,
    config: &mbus::config::AppConfig,
    steps: &[mbus::agent::memory::StepRecord],
    task_id: &str,
    run_timestamp: &str,
) -> Result<Option<mbus::output::OutputArtifact>, Box<dyn Error>> {
    let Some(path) = config.output.extract_output.as_ref() else {
        return Ok(None);
    };
    if let Some(output) = mbus::output::build_extract_output(task, task_id, run_timestamp, steps) {
        let record_count = Some(output.extracts.len());
        mbus::output::write_extract_output(path, &output)?;
        return Ok(Some(mbus::output::OutputArtifact {
            kind: "extract_output".to_string(),
            path: path.display().to_string(),
            record_count,
            step_index: None,
            artifact_ref: None,
            mime_type: None,
            sha256: None,
            bytes: None,
        }));
    }
    Ok(None)
}

fn write_transition_trace(
    run_id: &str,
    task: &str,
    steps: &[mbus::agent::memory::StepRecord],
    task_id: &str,
    run_timestamp: &str,
) -> Result<Option<mbus::output::OutputArtifact>, Box<dyn Error>> {
    let Some(trace) = mbus::output::build_transition_trace(task, task_id, run_timestamp, steps)
    else {
        return Ok(None);
    };
    let artifact = mbus::output::write_transition_trace_artifact(run_id, &trace)?;
    Ok(Some(artifact))
}

fn write_screenshot_artifacts(
    config: &mbus::config::AppConfig,
    run_id: &str,
    terminal_state: &mbus::output::TerminalState,
    step_screenshots: &[Option<Vec<u8>>],
) -> ScreenshotPersistResult {
    if !config.screenshot.enabled {
        return ScreenshotPersistResult::default();
    }
    if !should_persist_screenshots(&config.screenshot.persist, terminal_state) {
        return ScreenshotPersistResult::default();
    }

    let mut artifacts = Vec::new();
    let mut errors = Vec::new();
    for (index, screenshot) in step_screenshots.iter().enumerate() {
        let Some(bytes) = screenshot.as_ref() else {
            continue;
        };
        match mbus::output::write_screenshot_artifact(run_id, index + 1, bytes) {
            Ok(artifact) => artifacts.push(artifact),
            Err(err) => {
                telemetry::inc_screenshot_persist_failure();
                errors.push(mbus::output::RunErrorSummary {
                    code: "screenshot_persist_failed".to_string(),
                    message: err.to_string(),
                    step_index: Some(index + 1),
                    field: None,
                    validation_code: None,
                    kind: Some("screenshot".to_string()),
                });
            }
        }
    }
    ScreenshotPersistResult { artifacts, errors }
}

fn should_persist_screenshots(
    policy: &mbus::config::ScreenshotPersist,
    terminal_state: &mbus::output::TerminalState,
) -> bool {
    match policy {
        mbus::config::ScreenshotPersist::None => false,
        mbus::config::ScreenshotPersist::Always => true,
        mbus::config::ScreenshotPersist::OnError => {
            !matches!(terminal_state, mbus::output::TerminalState::Done)
        }
    }
}

fn emit_json<T: Serialize>(value: &T) -> Result<(), Box<dyn Error>> {
    let text = serde_json::to_string(value)?;
    println!("{text}");
    Ok(())
}

#[derive(Serialize)]
struct StepLog {
    #[serde(rename = "type")]
    r#type: &'static str,
    index: usize,
    action: mbus::types::Action,
    validation: mbus::agent::memory::ValidationOutcome,
    result: mbus::types::StepResult,
    outcome: mbus::agent::memory::StepOutcomeLog,
    timings: mbus::agent::memory::StepTimings,
    llm_payload_mode: mbus::types::LlmPayloadMode,
    image_context_sent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    llm_usage: Option<mbus::types::TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    router: Option<mbus::agent::memory::RouterStepInfo>,
}

#[derive(Serialize)]
struct SummaryLog {
    #[serde(rename = "type")]
    r#type: &'static str,
    status: &'static str,
    terminal_state: mbus::output::TerminalState,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_action: Option<mbus::types::Action>,
    steps: usize,
    validation_failures: usize,
    apply_failures: usize,
    apply_successes: usize,
    done_steps: usize,
    repair_attempts: usize,
    repair_successes: usize,
    repair_failures: usize,
    screenshots: mbus::output::ScreenshotSummary,
    router: mbus::output::RouterSummary,
    errors: Vec<mbus::output::RunErrorSummary>,
    output_artifacts: Vec<mbus::output::OutputArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_title: Option<String>,
}

fn repair_counts_delta(
    start: &telemetry::MetricsSnapshot,
    end: &telemetry::MetricsSnapshot,
) -> mbus::output::RepairCounts {
    mbus::output::RepairCounts {
        attempts: saturating_delta(start.repair_attempts_total, end.repair_attempts_total),
        successes: saturating_delta(start.repair_success_total, end.repair_success_total),
        failures: saturating_delta(start.repair_failures_total, end.repair_failures_total),
    }
}

fn screenshot_counts_delta(
    start: &telemetry::MetricsSnapshot,
    end: &telemetry::MetricsSnapshot,
) -> mbus::output::ScreenshotSummary {
    mbus::output::ScreenshotSummary {
        captures: saturating_delta(
            start.screenshot_captures_total,
            end.screenshot_captures_total,
        ),
        failures: saturating_delta(
            start.screenshot_failures_total,
            end.screenshot_failures_total,
        ),
        bytes: saturating_delta(start.screenshot_bytes_total, end.screenshot_bytes_total),
        duration_ms: saturating_delta(
            start.screenshot_duration_ms_total,
            end.screenshot_duration_ms_total,
        ),
        persist_failures: saturating_delta(
            start.screenshot_persist_failures_total,
            end.screenshot_persist_failures_total,
        ),
    }
}

fn saturating_delta(start: u64, end: u64) -> usize {
    let delta = end.saturating_sub(start);
    usize::try_from(delta).unwrap_or(usize::MAX)
}

#[derive(Serialize)]
struct ConfigLog {
    #[serde(rename = "type")]
    r#type: &'static str,
    agent: AgentLog,
    browser: BrowserLog,
    router: RouterLog,
    validator: ValidatorLog,
    llm: LlmLog,
    output: OutputLog,
    screenshot: ScreenshotLog,
}

impl From<&mbus::config::AppConfig> for ConfigLog {
    fn from(config: &mbus::config::AppConfig) -> Self {
        Self {
            r#type: "config",
            agent: AgentLog {
                max_steps: config.agent.max_steps,
                max_no_progress_steps: config.agent.max_no_progress_steps,
                memory_max_observations: config.agent.memory.max_observations,
                memory_max_history: config.agent.memory.max_history,
            },
            browser: BrowserLog {
                headful: config.browser.headful,
                initial_url: config.browser.initial_url.clone(),
                cdp_url: config
                    .browser
                    .cdp_url
                    .as_ref()
                    .map(|value| redact_url(value)),
                snapshot_timeout_ms: config.browser.snapshot_timeout.as_millis() as u64,
                action_timeout_ms: config.browser.action_timeout.as_millis() as u64,
                max_elements: config.browser.max_elements,
                max_text_len: config.browser.max_text_len,
            },
            router: RouterLog {
                failures_to_mid: config.router.failures_to_mid,
                failures_to_strong: config.router.failures_to_strong,
                no_progress_to_mid: config.router.no_progress_to_mid,
                no_progress_to_strong: config.router.no_progress_to_strong,
            },
            validator: ValidatorLog {
                allow_insecure: config.validator.allow_insecure,
                max_text_len: config.validator.max_text_len,
                max_wait_ms: config.validator.max_wait_ms,
                max_scroll: config.validator.max_scroll,
            },
            llm: LlmLog {
                mode: format!("{:?}", config.llm.mode).to_lowercase(),
                base_url: redact_url(&config.llm.base_url),
                model_fast: config.llm.model_fast.clone(),
                model_mid: config.llm.model_mid.clone(),
                model_strong: config.llm.model_strong.clone(),
                timeout_ms: config.llm.timeout_ms,
                temperature: config.llm.temperature,
                max_tokens: config.llm.max_tokens,
                actions_file: config
                    .llm
                    .actions_file
                    .as_ref()
                    .map(|value| value.display().to_string()),
                api_key_present: config.llm.api_key.is_some(),
            },
            output: OutputLog {
                extract_output: config
                    .output
                    .extract_output
                    .as_ref()
                    .map(|value| value.display().to_string()),
            },
            screenshot: ScreenshotLog {
                enabled: config.screenshot.enabled,
                persist: config.screenshot.persist.as_str().to_string(),
            },
        }
    }
}

fn redact_url(value: &str) -> String {
    let Some(scheme_end) = value.find("://") else {
        return value.to_string();
    };
    let userinfo_start = scheme_end + 3;
    let rest = &value[userinfo_start..];
    let Some(at_index) = rest.find('@') else {
        return value.to_string();
    };
    let prefix = &value[..userinfo_start];
    let suffix = &rest[at_index + 1..];
    format!("{prefix}***@{suffix}")
}

#[derive(Serialize)]
struct AgentLog {
    max_steps: usize,
    max_no_progress_steps: usize,
    memory_max_observations: usize,
    memory_max_history: usize,
}

#[derive(Serialize)]
struct BrowserLog {
    headful: bool,
    initial_url: String,
    cdp_url: Option<String>,
    snapshot_timeout_ms: u64,
    action_timeout_ms: u64,
    max_elements: usize,
    max_text_len: usize,
}

#[derive(Serialize)]
struct RouterLog {
    failures_to_mid: u32,
    failures_to_strong: u32,
    no_progress_to_mid: u32,
    no_progress_to_strong: u32,
}

#[derive(Serialize)]
struct ValidatorLog {
    allow_insecure: bool,
    max_text_len: usize,
    max_wait_ms: u64,
    max_scroll: i64,
}

#[derive(Serialize)]
struct LlmLog {
    mode: String,
    base_url: String,
    model_fast: String,
    model_mid: String,
    model_strong: String,
    timeout_ms: u64,
    temperature: f32,
    max_tokens: Option<u32>,
    actions_file: Option<String>,
    api_key_present: bool,
}

#[derive(Serialize)]
struct OutputLog {
    extract_output: Option<String>,
}

#[derive(Serialize)]
struct ScreenshotLog {
    enabled: bool,
    persist: String,
}
#[derive(Serialize)]
struct BenchConfigLog {
    #[serde(rename = "type")]
    r#type: &'static str,
    tasks_dir: String,
    report_path: String,
    max_steps_per_task: usize,
    required_passes: usize,
    base_url: String,
}

#[derive(Serialize)]
struct BenchTaskLog {
    #[serde(rename = "type")]
    r#type: &'static str,
    task_id: String,
    passed: bool,
    status: BenchObservedStatus,
    steps: usize,
    duration_ms: u64,
    usage: BenchTokenUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_reason: Option<String>,
}

impl From<&BenchTaskResult> for BenchTaskLog {
    fn from(value: &BenchTaskResult) -> Self {
        Self {
            r#type: "bench_task",
            task_id: value.task_id.clone(),
            passed: value.passed,
            status: value.status,
            steps: value.steps,
            duration_ms: value.duration_ms,
            usage: value.usage.clone(),
            failure_reason: value.failure_reason.clone(),
        }
    }
}

#[derive(Serialize)]
struct BenchSummaryLog {
    #[serde(rename = "type")]
    r#type: &'static str,
    total_tasks: usize,
    passed_tasks: usize,
    required_passes: usize,
    completion_rate: f64,
    median_steps_success: Option<u64>,
    p95_steps_success: Option<u64>,
    gate_passed: bool,
    failure_buckets: std::collections::BTreeMap<String, usize>,
    report_path: String,
}

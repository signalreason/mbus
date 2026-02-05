use clap::{Args, Parser, Subcommand};
use mbus::agent::r#loop::{AgentLoop, LlmClients, RunStatus};
use mbus::browser::CdpBrowser;
use mbus::config::{load_config, CliOverrides, ConfigError, LlmConfig, LlmMode};
use mbus::llm::openai::{OpenAiClient, OpenAiConfig};
use mbus::llm::router::Router;
use mbus::llm::scripted::{ScriptedLlm, StubLlm};
use mbus::verify::rules::Validator;
use serde::Serialize;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "mbus", version, about = "Rust browser + LLM agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Run(RunArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    #[arg(long)]
    task: Option<String>,
    #[arg(long)]
    task_file: Option<PathBuf>,
    #[arg(long)]
    plan: Option<String>,
    #[arg(long)]
    plan_file: Option<PathBuf>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    headless: Option<bool>,
    #[arg(long)]
    initial_url: Option<String>,
    #[arg(long)]
    max_steps: Option<usize>,
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
}

#[tokio::main]
async fn main() {
    if let Err(err) = run_cli().await {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

async fn run_cli() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run(args) => run_command(args).await,
    }
}

async fn run_command(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let task = resolve_required_text("task", args.task.as_deref(), args.task_file.as_deref())?;
    let plan = resolve_optional_text("plan", args.plan.as_deref(), args.plan_file.as_deref())?;

    let config_path = resolve_config_path(args.config.as_deref());
    let cli_overrides = build_cli_overrides(&args)?;
    let config = load_config(config_path.as_deref(), cli_overrides)?;

    emit_json(&ConfigLog::from(&config))?;

    let browser = CdpBrowser::launch(config.browser.clone()).await?;
    let clients = build_clients(&config.llm)?;

    let mut agent = AgentLoop::new(browser, clients, task);
    if let Some(plan) = plan.as_ref() {
        agent = agent.with_plan(plan.to_string());
    }
    agent = agent
        .with_policy(config.agent.clone())
        .with_router(Router::new(config.router.clone()))
        .with_validator(Validator::new(config.validator.clone()));

    let run_result = agent.run().await;
    let shutdown_result = agent.shutdown().await;

    if let Err(err) = shutdown_result {
        eprintln!("shutdown error: {err}");
    }

    let result = run_result?;
    emit_run_logs(&result)?;

    Ok(())
}

fn resolve_config_path(cli_path: Option<&Path>) -> Option<PathBuf> {
    cli_path.map(|path| path.to_path_buf()).or_else(|| {
        std::env::var("MBUS_CONFIG")
            .ok()
            .map(PathBuf::from)
    })
}

fn resolve_required_text(
    label: &str,
    inline: Option<&str>,
    file_path: Option<&Path>,
) -> Result<String, Box<dyn Error>> {
    match (inline, file_path) {
        (Some(_), Some(_)) => Err(format!("{label}: use only one of --{label} or --{label}-file").into()),
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
        (Some(_), Some(_)) => Err(format!("{label}: use only one of --{label} or --{label}-file").into()),
        (Some(value), None) => Ok(Some(value.to_string())),
        (None, Some(path)) => {
            let content = std::fs::read_to_string(path)?;
            Ok(Some(content.trim().to_string()))
        }
        (None, None) => Ok(None),
    }
}

fn build_cli_overrides(args: &RunArgs) -> Result<CliOverrides, ConfigError> {
    let llm_mode = if let Some(mode) = args.llm_mode.as_deref() {
        Some(LlmMode::from_str(mode)?)
    } else if args.llm_actions_file.is_some() {
        Some(LlmMode::Scripted)
    } else {
        None
    };

    Ok(CliOverrides {
        max_steps: args.max_steps,
        memory_max_observations: args.memory_max_observations,
        memory_max_history: args.memory_max_history,
        headless: args.headless,
        initial_url: args.initial_url.clone(),
        snapshot_timeout_ms: args.snapshot_timeout_ms,
        action_timeout_ms: args.action_timeout_ms,
        max_elements: args.max_elements,
        max_text_len: args.max_text_len,
        router_failures_to_mid: args.router_failures_to_mid,
        router_failures_to_strong: args.router_failures_to_strong,
        router_no_progress_to_mid: args.router_no_progress_to_mid,
        router_no_progress_to_strong: args.router_no_progress_to_strong,
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
        llm_actions_file: args.llm_actions_file.clone(),
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
                .ok_or_else(|| "llm.actions_file is required for scripted mode")?;
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
                .ok_or_else(|| "llm.api_key is required for openai mode")?;
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

fn emit_run_logs(result: &mbus::agent::r#loop::RunResult) -> Result<(), Box<dyn Error>> {
    for (index, step) in result.steps.iter().enumerate() {
        emit_json(&StepLog {
            r#type: "step",
            index: index + 1,
            action: step.action.clone(),
            result: step.result.clone(),
        })?;
    }

    let status = match result.status {
        RunStatus::Done => "done",
        RunStatus::MaxSteps => "max_steps",
    };

    emit_json(&SummaryLog {
        r#type: "summary",
        status,
        final_action: result.final_action.clone(),
        steps: result.steps.len(),
        final_url: result.final_observation.url.clone(),
        final_title: result.final_observation.title.clone(),
    })?;

    Ok(())
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
    result: mbus::types::StepResult,
}

#[derive(Serialize)]
struct SummaryLog {
    #[serde(rename = "type")]
    r#type: &'static str,
    status: &'static str,
    final_action: mbus::types::Action,
    steps: usize,
    final_url: String,
    final_title: String,
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
}

impl From<&mbus::config::AppConfig> for ConfigLog {
    fn from(config: &mbus::config::AppConfig) -> Self {
        Self {
            r#type: "config",
            agent: AgentLog {
                max_steps: config.agent.max_steps,
                memory_max_observations: config.agent.memory.max_observations,
                memory_max_history: config.agent.memory.max_history,
            },
            browser: BrowserLog {
                headful: config.browser.headful,
                initial_url: config.browser.initial_url.clone(),
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
                base_url: config.llm.base_url.clone(),
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
        }
    }
}

#[derive(Serialize)]
struct AgentLog {
    max_steps: usize,
    memory_max_observations: usize,
    memory_max_history: usize,
}

#[derive(Serialize)]
struct BrowserLog {
    headful: bool,
    initial_url: String,
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

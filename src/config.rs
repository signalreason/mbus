use crate::agent::policy::AgentPolicy;
use crate::browser::CdpConfig;
use crate::llm::router::{RouterConfig, RouterLadderStep, Tier};
use crate::types::ReasoningEffort;
use crate::verify::rules::ValidatorConfig;
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct AppConfig {
    pub agent: AgentPolicy,
    pub browser: CdpConfig,
    pub router: RouterConfig,
    pub validator: ValidatorConfig,
    pub llm: LlmConfig,
    pub output: OutputConfig,
    pub screenshot: ScreenshotConfig,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LlmMode {
    #[default]
    Stub,
    OpenAi,
    Scripted,
}

impl FromStr for LlmMode {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "stub" => Ok(LlmMode::Stub),
            "openai" => Ok(LlmMode::OpenAi),
            "scripted" => Ok(LlmMode::Scripted),
            other => Err(ConfigError::invalid(format!("unknown llm mode '{other}'"))),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub mode: LlmMode,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model_fast: String,
    pub model_mid: String,
    pub model_strong: String,
    pub timeout_ms: u64,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub input_cost_per_million: Option<f64>,
    pub output_cost_per_million: Option<f64>,
    pub actions_file: Option<PathBuf>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            mode: LlmMode::default(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: None,
            model_fast: "gpt-5-mini".to_string(),
            model_mid: "gpt-5.1".to_string(),
            model_strong: "gpt-5.2".to_string(),
            timeout_ms: 30_000,
            temperature: 1.0,
            max_tokens: Some(256),
            input_cost_per_million: None,
            output_cost_per_million: None,
            actions_file: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OutputConfig {
    pub extract_output: Option<PathBuf>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            extract_output: Some(PathBuf::from("mbus_extract.json")),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ScreenshotPersist {
    #[default]
    None,
    OnError,
    Always,
}

impl ScreenshotPersist {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScreenshotPersist::None => "none",
            ScreenshotPersist::OnError => "on_error",
            ScreenshotPersist::Always => "always",
        }
    }
}

impl FromStr for ScreenshotPersist {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_lowercase().as_str() {
            "none" | "off" | "disabled" => Ok(ScreenshotPersist::None),
            "on_error" | "on-error" | "error" => Ok(ScreenshotPersist::OnError),
            "always" | "all" => Ok(ScreenshotPersist::Always),
            other => Err(ConfigError::invalid(format!(
                "unknown screenshot persist mode '{other}'"
            ))),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScreenshotConfig {
    pub enabled: bool,
    pub persist: ScreenshotPersist,
}

#[derive(Clone, Debug, Default)]
pub struct CliOverrides {
    pub max_steps: Option<usize>,
    pub max_no_progress_steps: Option<usize>,
    pub memory_max_observations: Option<usize>,
    pub memory_max_history: Option<usize>,
    pub headless: Option<bool>,
    pub initial_url: Option<String>,
    pub cdp_url: Option<String>,
    pub browser_executable: Option<PathBuf>,
    pub browser_launch_timeout_ms: Option<u64>,
    pub browser_no_sandbox: Option<bool>,
    pub browser_args: Option<Vec<String>>,
    pub browser_keep_user_data_dir: Option<bool>,
    pub snapshot_timeout_ms: Option<u64>,
    pub action_timeout_ms: Option<u64>,
    pub max_elements: Option<usize>,
    pub max_text_len: Option<usize>,
    pub router_failures_to_mid: Option<u32>,
    pub router_failures_to_strong: Option<u32>,
    pub router_no_progress_to_mid: Option<u32>,
    pub router_no_progress_to_strong: Option<u32>,
    pub router_reasoning_effort: Option<ReasoningEffort>,
    pub router_ladder: Option<Vec<String>>,
    pub allow_insecure: Option<bool>,
    pub validator_max_text_len: Option<usize>,
    pub validator_max_wait_ms: Option<u64>,
    pub validator_max_scroll: Option<i64>,
    pub llm_mode: Option<LlmMode>,
    pub llm_base_url: Option<String>,
    pub llm_api_key: Option<String>,
    pub llm_model_fast: Option<String>,
    pub llm_model_mid: Option<String>,
    pub llm_model_strong: Option<String>,
    pub llm_timeout_ms: Option<u64>,
    pub llm_temperature: Option<f32>,
    pub llm_max_tokens: Option<u32>,
    pub llm_input_cost_per_million: Option<f64>,
    pub llm_output_cost_per_million: Option<f64>,
    pub llm_actions_file: Option<PathBuf>,
    pub extract_output: Option<PathBuf>,
    pub screenshot_enabled: Option<bool>,
    pub screenshot_persist: Option<ScreenshotPersist>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io { path: PathBuf, message: String },
    Toml { path: PathBuf, message: String },
    Env { name: String, message: String },
    Invalid { message: String },
}

impl ConfigError {
    fn invalid(message: impl Into<String>) -> Self {
        ConfigError::Invalid {
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io { path, message } => {
                write!(f, "failed to read config {}: {message}", path.display())
            }
            ConfigError::Toml { path, message } => {
                write!(f, "failed to parse config {}: {message}", path.display())
            }
            ConfigError::Env { name, message } => {
                write!(f, "invalid env {name}: {message}")
            }
            ConfigError::Invalid { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn load_config(
    config_path: Option<&Path>,
    cli: CliOverrides,
) -> Result<AppConfig, ConfigError> {
    let mut config = AppConfig::default();

    if let Some(path) = config_path {
        let file_config = FileConfig::from_path(path)?;
        file_config.apply(&mut config)?;
    }

    let env_overrides = EnvOverrides::from_env()?;
    env_overrides.apply(&mut config)?;

    cli.apply(&mut config)?;

    normalize_router_ladder(&mut config)?;

    Ok(config)
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileConfig {
    #[serde(default)]
    agent: Option<FileAgentConfig>,
    #[serde(default)]
    browser: Option<FileBrowserConfig>,
    #[serde(default)]
    router: Option<FileRouterConfig>,
    #[serde(default)]
    validator: Option<FileValidatorConfig>,
    #[serde(default)]
    llm: Option<FileLlmConfig>,
    #[serde(default)]
    output: Option<FileOutputConfig>,
    #[serde(default)]
    screenshot: Option<FileScreenshotConfig>,
}

impl FileConfig {
    fn from_path(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|err| ConfigError::Io {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        toml::from_str(&content).map_err(|err| ConfigError::Toml {
            path: path.to_path_buf(),
            message: err.to_string(),
        })
    }

    fn apply(&self, config: &mut AppConfig) -> Result<(), ConfigError> {
        if let Some(agent) = self.agent.as_ref() {
            if let Some(max_steps) = agent.max_steps {
                config.agent.max_steps = max_steps;
            }
            if let Some(max_no_progress_steps) = agent.max_no_progress_steps {
                config.agent.max_no_progress_steps = max_no_progress_steps;
            }
            if let Some(memory) = agent.memory.as_ref() {
                if let Some(max_obs) = memory.max_observations {
                    config.agent.memory.max_observations = max_obs;
                }
                if let Some(max_history) = memory.max_history {
                    config.agent.memory.max_history = max_history;
                }
            }
        }

        if let Some(browser) = self.browser.as_ref() {
            apply_headless(&mut config.browser, browser.headless, browser.headful)?;
            if let Some(initial_url) = browser.initial_url.as_ref() {
                config.browser.initial_url = initial_url.to_string();
            }
            if let Some(cdp_url) = browser.cdp_url.as_ref() {
                config.browser.cdp_url = Some(cdp_url.to_string());
            }
            if let Some(path) = browser.executable_path.as_ref() {
                config.browser.executable_path = Some(PathBuf::from(path));
            }
            if let Some(timeout_ms) = browser.launch_timeout_ms {
                config.browser.launch_timeout = Duration::from_millis(timeout_ms);
            }
            if let Some(value) = browser.no_sandbox {
                config.browser.no_sandbox = value;
            }
            if let Some(value) = browser.extra_args.as_ref() {
                config.browser.extra_args = value.clone();
            }
            if let Some(value) = browser.keep_user_data_dir {
                config.browser.keep_user_data_dir = value;
            }
            if let Some(timeout_ms) = browser.snapshot_timeout_ms {
                config.browser.snapshot_timeout = Duration::from_millis(timeout_ms);
            }
            if let Some(timeout_ms) = browser.action_timeout_ms {
                config.browser.action_timeout = Duration::from_millis(timeout_ms);
            }
            if let Some(max_elements) = browser.max_elements {
                config.browser.max_elements = max_elements;
            }
            if let Some(max_text_len) = browser.max_text_len {
                config.browser.max_text_len = max_text_len;
            }
        }

        if let Some(path) = self
            .output
            .as_ref()
            .and_then(|output| output.extract_output.as_ref())
        {
            config.output.extract_output = Some(PathBuf::from(path));
        }

        if let Some(screenshot) = self.screenshot.as_ref() {
            if let Some(value) = screenshot.enabled {
                config.screenshot.enabled = value;
            }
            if let Some(value) = screenshot.persist.as_deref() {
                config.screenshot.persist = ScreenshotPersist::from_str(value)?;
            }
        }

        if let Some(router) = self.router.as_ref() {
            if let Some(value) = router.failures_to_mid {
                config.router.failures_to_mid = value;
            }
            if let Some(value) = router.failures_to_strong {
                config.router.failures_to_strong = value;
            }
            if let Some(value) = router.no_progress_to_mid {
                config.router.no_progress_to_mid = value;
            }
            if let Some(value) = router.no_progress_to_strong {
                config.router.no_progress_to_strong = value;
            }
            if let Some(value) = router.reasoning_effort.as_deref() {
                config.router.reasoning_effort =
                    parse_reasoning_effort(value).map_err(ConfigError::invalid)?;
            }
            if let Some(value) = router.ladder.as_ref() {
                config.router.ladder_spec = Some(value.clone());
            }
        }

        if let Some(validator) = self.validator.as_ref() {
            if let Some(value) = validator.allow_insecure {
                config.validator.allow_insecure = value;
            }
            if let Some(value) = validator.max_text_len {
                config.validator.max_text_len = value;
            }
            if let Some(value) = validator.max_wait_ms {
                config.validator.max_wait_ms = value;
            }
            if let Some(value) = validator.max_scroll {
                config.validator.max_scroll = value;
            }
        }

        if let Some(llm) = self.llm.as_ref() {
            if let Some(mode) = llm.mode.as_deref() {
                config.llm.mode = LlmMode::from_str(mode)?;
            }
            if let Some(base_url) = llm.base_url.as_ref() {
                config.llm.base_url = base_url.to_string();
            }
            if let Some(api_key) = llm.api_key.as_ref() {
                config.llm.api_key = Some(api_key.to_string());
            }
            if let Some(value) = llm.model_fast.as_ref() {
                config.llm.model_fast = value.to_string();
            }
            if let Some(value) = llm.model_mid.as_ref() {
                config.llm.model_mid = value.to_string();
            }
            if let Some(value) = llm.model_strong.as_ref() {
                config.llm.model_strong = value.to_string();
            }
            if let Some(value) = llm.timeout_ms {
                config.llm.timeout_ms = value;
            }
            if let Some(value) = llm.temperature {
                config.llm.temperature = value;
            }
            if let Some(value) = llm.max_tokens {
                config.llm.max_tokens = Some(value);
            }
            if let Some(value) = llm.input_cost_per_million {
                config.llm.input_cost_per_million = Some(value);
            }
            if let Some(value) = llm.output_cost_per_million {
                config.llm.output_cost_per_million = Some(value);
            }
            if let Some(path) = llm.actions_file.as_ref() {
                config.llm.actions_file = Some(PathBuf::from(path));
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileAgentConfig {
    max_steps: Option<usize>,
    max_no_progress_steps: Option<usize>,
    #[serde(default)]
    memory: Option<FileMemoryConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileMemoryConfig {
    max_observations: Option<usize>,
    max_history: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileBrowserConfig {
    headless: Option<bool>,
    headful: Option<bool>,
    initial_url: Option<String>,
    cdp_url: Option<String>,
    executable_path: Option<String>,
    launch_timeout_ms: Option<u64>,
    no_sandbox: Option<bool>,
    extra_args: Option<Vec<String>>,
    keep_user_data_dir: Option<bool>,
    snapshot_timeout_ms: Option<u64>,
    action_timeout_ms: Option<u64>,
    max_elements: Option<usize>,
    max_text_len: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileRouterConfig {
    failures_to_mid: Option<u32>,
    failures_to_strong: Option<u32>,
    no_progress_to_mid: Option<u32>,
    no_progress_to_strong: Option<u32>,
    reasoning_effort: Option<String>,
    ladder: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileValidatorConfig {
    allow_insecure: Option<bool>,
    max_text_len: Option<usize>,
    max_wait_ms: Option<u64>,
    max_scroll: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileLlmConfig {
    mode: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    model_fast: Option<String>,
    model_mid: Option<String>,
    model_strong: Option<String>,
    timeout_ms: Option<u64>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    input_cost_per_million: Option<f64>,
    output_cost_per_million: Option<f64>,
    actions_file: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileOutputConfig {
    extract_output: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct FileScreenshotConfig {
    enabled: Option<bool>,
    persist: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct EnvOverrides {
    inner: CliOverrides,
}

impl EnvOverrides {
    fn from_env() -> Result<Self, ConfigError> {
        let vars = std::env::vars();
        Self::from_pairs(vars)
    }

    fn from_pairs<I>(pairs: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut overrides = CliOverrides::default();

        for (key, value) in pairs {
            if !key.starts_with("MBUS_") {
                continue;
            }
            match key.as_str() {
                "MBUS_MAX_STEPS" => overrides.max_steps = Some(parse_usize(&key, &value)?),
                "MBUS_MAX_NO_PROGRESS_STEPS" => {
                    overrides.max_no_progress_steps = Some(parse_usize(&key, &value)?)
                }
                "MBUS_MEMORY_MAX_OBSERVATIONS" => {
                    overrides.memory_max_observations = Some(parse_usize(&key, &value)?)
                }
                "MBUS_MEMORY_MAX_HISTORY" => {
                    overrides.memory_max_history = Some(parse_usize(&key, &value)?)
                }
                "MBUS_HEADLESS" => overrides.headless = Some(parse_bool(&key, &value)?),
                "MBUS_INITIAL_URL" => overrides.initial_url = Some(value),
                "MBUS_CDP_URL" => overrides.cdp_url = Some(value),
                "MBUS_BROWSER_EXECUTABLE" => {
                    overrides.browser_executable = Some(PathBuf::from(value))
                }
                "MBUS_BROWSER_LAUNCH_TIMEOUT_MS" => {
                    overrides.browser_launch_timeout_ms = Some(parse_u64(&key, &value)?)
                }
                "MBUS_BROWSER_NO_SANDBOX" => {
                    overrides.browser_no_sandbox = Some(parse_bool(&key, &value)?)
                }
                "MBUS_BROWSER_EXTRA_ARGS" => {
                    overrides.browser_args = Some(parse_string_list_env(&key, &value)?)
                }
                "MBUS_BROWSER_KEEP_USER_DATA_DIR" => {
                    overrides.browser_keep_user_data_dir = Some(parse_bool(&key, &value)?)
                }
                "MBUS_SNAPSHOT_TIMEOUT_MS" => {
                    overrides.snapshot_timeout_ms = Some(parse_u64(&key, &value)?)
                }
                "MBUS_ACTION_TIMEOUT_MS" => {
                    overrides.action_timeout_ms = Some(parse_u64(&key, &value)?)
                }
                "MBUS_MAX_ELEMENTS" => overrides.max_elements = Some(parse_usize(&key, &value)?),
                "MBUS_MAX_TEXT_LEN" => overrides.max_text_len = Some(parse_usize(&key, &value)?),
                "MBUS_ROUTER_FAILURES_TO_MID" => {
                    overrides.router_failures_to_mid = Some(parse_u32(&key, &value)?)
                }
                "MBUS_ROUTER_FAILURES_TO_STRONG" => {
                    overrides.router_failures_to_strong = Some(parse_u32(&key, &value)?)
                }
                "MBUS_ROUTER_NO_PROGRESS_TO_MID" => {
                    overrides.router_no_progress_to_mid = Some(parse_u32(&key, &value)?)
                }
                "MBUS_ROUTER_NO_PROGRESS_TO_STRONG" => {
                    overrides.router_no_progress_to_strong = Some(parse_u32(&key, &value)?)
                }
                "MBUS_ROUTER_REASONING_EFFORT" => {
                    overrides.router_reasoning_effort = Some(
                        parse_reasoning_effort(&value)
                            .map_err(|message| ConfigError::Env { name: key, message })?,
                    );
                }
                "MBUS_ROUTER_LADDER" => {
                    overrides.router_ladder = Some(parse_router_ladder_env(&key, &value)?);
                }
                "MBUS_ALLOW_INSECURE" => overrides.allow_insecure = Some(parse_bool(&key, &value)?),
                "MBUS_VALIDATOR_MAX_TEXT_LEN" => {
                    overrides.validator_max_text_len = Some(parse_usize(&key, &value)?)
                }
                "MBUS_VALIDATOR_MAX_WAIT_MS" => {
                    overrides.validator_max_wait_ms = Some(parse_u64(&key, &value)?)
                }
                "MBUS_VALIDATOR_MAX_SCROLL" => {
                    overrides.validator_max_scroll = Some(parse_i64(&key, &value)?)
                }
                "MBUS_LLM_MODE" => overrides.llm_mode = Some(LlmMode::from_str(&value)?),
                "MBUS_LLM_BASE_URL" => overrides.llm_base_url = Some(value),
                "MBUS_LLM_API_KEY" => overrides.llm_api_key = Some(value),
                "MBUS_LLM_MODEL_FAST" => overrides.llm_model_fast = Some(value),
                "MBUS_LLM_MODEL_MID" => overrides.llm_model_mid = Some(value),
                "MBUS_LLM_MODEL_STRONG" => overrides.llm_model_strong = Some(value),
                "MBUS_LLM_TIMEOUT_MS" => overrides.llm_timeout_ms = Some(parse_u64(&key, &value)?),
                "MBUS_LLM_TEMPERATURE" => {
                    overrides.llm_temperature = Some(parse_f32(&key, &value)?)
                }
                "MBUS_LLM_MAX_TOKENS" => overrides.llm_max_tokens = Some(parse_u32(&key, &value)?),
                "MBUS_LLM_INPUT_COST_PER_MILLION" => {
                    overrides.llm_input_cost_per_million = Some(parse_f64(&key, &value)?)
                }
                "MBUS_LLM_OUTPUT_COST_PER_MILLION" => {
                    overrides.llm_output_cost_per_million = Some(parse_f64(&key, &value)?)
                }
                "MBUS_LLM_ACTIONS_FILE" => overrides.llm_actions_file = Some(PathBuf::from(value)),
                "MBUS_EXTRACT_OUTPUT" => overrides.extract_output = Some(PathBuf::from(value)),
                "MBUS_SCREENSHOT_ENABLED" => {
                    overrides.screenshot_enabled = Some(parse_bool(&key, &value)?)
                }
                "MBUS_SCREENSHOT_PERSIST" => {
                    overrides.screenshot_persist = Some(ScreenshotPersist::from_str(&value)?)
                }
                _ => {}
            }
        }

        Ok(Self { inner: overrides })
    }

    fn apply(&self, config: &mut AppConfig) -> Result<(), ConfigError> {
        self.inner.apply(config)
    }
}

impl CliOverrides {
    fn apply(&self, config: &mut AppConfig) -> Result<(), ConfigError> {
        if let Some(max_steps) = self.max_steps {
            config.agent.max_steps = max_steps;
        }
        if let Some(max_no_progress_steps) = self.max_no_progress_steps {
            config.agent.max_no_progress_steps = max_no_progress_steps;
        }
        if let Some(value) = self.memory_max_observations {
            config.agent.memory.max_observations = value;
        }
        if let Some(value) = self.memory_max_history {
            config.agent.memory.max_history = value;
        }
        apply_headless(&mut config.browser, self.headless, None)?;
        if let Some(initial_url) = self.initial_url.as_ref() {
            config.browser.initial_url = initial_url.to_string();
        }
        if let Some(cdp_url) = self.cdp_url.as_ref() {
            config.browser.cdp_url = Some(cdp_url.to_string());
        }
        if let Some(value) = self.browser_executable.as_ref() {
            config.browser.executable_path = Some(value.to_path_buf());
        }
        if let Some(value) = self.browser_launch_timeout_ms {
            config.browser.launch_timeout = Duration::from_millis(value);
        }
        if let Some(value) = self.browser_no_sandbox {
            config.browser.no_sandbox = value;
        }
        if let Some(value) = self.browser_args.as_ref() {
            config.browser.extra_args = value.clone();
        }
        if let Some(value) = self.browser_keep_user_data_dir {
            config.browser.keep_user_data_dir = value;
        }
        if let Some(timeout_ms) = self.snapshot_timeout_ms {
            config.browser.snapshot_timeout = Duration::from_millis(timeout_ms);
        }
        if let Some(timeout_ms) = self.action_timeout_ms {
            config.browser.action_timeout = Duration::from_millis(timeout_ms);
        }
        if let Some(value) = self.max_elements {
            config.browser.max_elements = value;
        }
        if let Some(value) = self.max_text_len {
            config.browser.max_text_len = value;
        }
        if let Some(value) = self.router_failures_to_mid {
            config.router.failures_to_mid = value;
        }
        if let Some(value) = self.router_failures_to_strong {
            config.router.failures_to_strong = value;
        }
        if let Some(value) = self.router_no_progress_to_mid {
            config.router.no_progress_to_mid = value;
        }
        if let Some(value) = self.router_no_progress_to_strong {
            config.router.no_progress_to_strong = value;
        }
        if let Some(value) = self.router_reasoning_effort {
            config.router.reasoning_effort = value;
        }
        if let Some(value) = self.router_ladder.as_ref() {
            config.router.ladder_spec = Some(value.clone());
        }
        if let Some(value) = self.allow_insecure {
            config.validator.allow_insecure = value;
        }
        if let Some(value) = self.validator_max_text_len {
            config.validator.max_text_len = value;
        }
        if let Some(value) = self.validator_max_wait_ms {
            config.validator.max_wait_ms = value;
        }
        if let Some(value) = self.validator_max_scroll {
            config.validator.max_scroll = value;
        }
        if let Some(mode) = self.llm_mode.clone() {
            config.llm.mode = mode;
        }
        if let Some(value) = self.llm_base_url.as_ref() {
            config.llm.base_url = value.to_string();
        }
        if let Some(value) = self.llm_api_key.as_ref() {
            config.llm.api_key = Some(value.to_string());
        }
        if let Some(value) = self.llm_model_fast.as_ref() {
            config.llm.model_fast = value.to_string();
        }
        if let Some(value) = self.llm_model_mid.as_ref() {
            config.llm.model_mid = value.to_string();
        }
        if let Some(value) = self.llm_model_strong.as_ref() {
            config.llm.model_strong = value.to_string();
        }
        if let Some(value) = self.llm_timeout_ms {
            config.llm.timeout_ms = value;
        }
        if let Some(value) = self.llm_temperature {
            config.llm.temperature = value;
        }
        if let Some(value) = self.llm_max_tokens {
            config.llm.max_tokens = Some(value);
        }
        if let Some(value) = self.llm_input_cost_per_million {
            config.llm.input_cost_per_million = Some(value);
        }
        if let Some(value) = self.llm_output_cost_per_million {
            config.llm.output_cost_per_million = Some(value);
        }
        if let Some(value) = self.llm_actions_file.as_ref() {
            config.llm.actions_file = Some(value.to_path_buf());
        }
        if let Some(value) = self.extract_output.as_ref() {
            config.output.extract_output = Some(value.to_path_buf());
        }
        if let Some(value) = self.screenshot_enabled {
            config.screenshot.enabled = value;
        }
        if let Some(value) = self.screenshot_persist.as_ref() {
            config.screenshot.persist = value.clone();
        }

        Ok(())
    }
}

fn apply_headless(
    config: &mut CdpConfig,
    headless: Option<bool>,
    headful: Option<bool>,
) -> Result<(), ConfigError> {
    if headless.is_some() && headful.is_some() {
        return Err(ConfigError::invalid("cannot set both headless and headful"));
    }
    if let Some(headless) = headless {
        config.headful = !headless;
    }
    if let Some(headful) = headful {
        config.headful = headful;
    }
    Ok(())
}

fn parse_bool(name: &str, value: &str) -> Result<bool, ConfigError> {
    match value.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(ConfigError::Env {
            name: name.to_string(),
            message: format!("expected bool, got '{value}'"),
        }),
    }
}

fn parse_usize(name: &str, value: &str) -> Result<usize, ConfigError> {
    value.parse::<usize>().map_err(|err| ConfigError::Env {
        name: name.to_string(),
        message: err.to_string(),
    })
}

fn parse_u64(name: &str, value: &str) -> Result<u64, ConfigError> {
    value.parse::<u64>().map_err(|err| ConfigError::Env {
        name: name.to_string(),
        message: err.to_string(),
    })
}

fn parse_u32(name: &str, value: &str) -> Result<u32, ConfigError> {
    value.parse::<u32>().map_err(|err| ConfigError::Env {
        name: name.to_string(),
        message: err.to_string(),
    })
}

fn parse_i64(name: &str, value: &str) -> Result<i64, ConfigError> {
    value.parse::<i64>().map_err(|err| ConfigError::Env {
        name: name.to_string(),
        message: err.to_string(),
    })
}

fn parse_f32(name: &str, value: &str) -> Result<f32, ConfigError> {
    value.parse::<f32>().map_err(|err| ConfigError::Env {
        name: name.to_string(),
        message: err.to_string(),
    })
}

fn parse_f64(name: &str, value: &str) -> Result<f64, ConfigError> {
    value.parse::<f64>().map_err(|err| ConfigError::Env {
        name: name.to_string(),
        message: err.to_string(),
    })
}

fn parse_string_list_env(name: &str, value: &str) -> Result<Vec<String>, ConfigError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.starts_with('[') {
        return serde_json::from_str::<Vec<String>>(trimmed).map_err(|err| ConfigError::Env {
            name: name.to_string(),
            message: err.to_string(),
        });
    }
    Ok(trimmed
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToString::to_string)
        .collect())
}

fn parse_reasoning_effort(value: &str) -> Result<ReasoningEffort, String> {
    ReasoningEffort::from_str(value)
}

fn parse_router_ladder_env(name: &str, value: &str) -> Result<Vec<String>, ConfigError> {
    let entries: Vec<String> = value
        .split(',')
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.to_string())
        .collect();
    if entries.is_empty() {
        return Err(ConfigError::Env {
            name: name.to_string(),
            message: "expected comma-separated model:effort entries".to_string(),
        });
    }
    Ok(entries)
}

fn normalize_router_ladder(config: &mut AppConfig) -> Result<(), ConfigError> {
    let ladder_spec = config.router.ladder_spec.take();
    let steps = if let Some(spec) = ladder_spec {
        if spec.is_empty() {
            return Err(ConfigError::invalid("router ladder must not be empty"));
        }
        parse_router_ladder(&spec, &config.llm)?
    } else if config.router.ladder.is_empty() {
        let effort = config.router.reasoning_effort;
        vec![
            RouterLadderStep {
                model: config.llm.model_fast.clone(),
                tier: Tier::Fast,
                effort,
            },
            RouterLadderStep {
                model: config.llm.model_mid.clone(),
                tier: Tier::Mid,
                effort,
            },
            RouterLadderStep {
                model: config.llm.model_strong.clone(),
                tier: Tier::Strong,
                effort,
            },
        ]
    } else {
        config.router.ladder.clone()
    };

    validate_router_ladder(&steps)?;
    config.router.ladder = steps;
    Ok(())
}

fn parse_router_ladder(
    spec: &[String],
    llm: &LlmConfig,
) -> Result<Vec<RouterLadderStep>, ConfigError> {
    spec.iter()
        .map(|entry| parse_router_ladder_entry(entry, llm))
        .collect()
}

fn parse_router_ladder_entry(
    entry: &str,
    llm: &LlmConfig,
) -> Result<RouterLadderStep, ConfigError> {
    let (model, effort) = entry
        .split_once(':')
        .ok_or_else(|| ConfigError::invalid(format!("invalid router ladder entry '{entry}'")))?;
    let model = model.trim();
    let effort = effort.trim();
    if model.is_empty() || effort.is_empty() {
        return Err(ConfigError::invalid(format!(
            "invalid router ladder entry '{entry}'"
        )));
    }
    let effort = ReasoningEffort::from_str(effort).map_err(|message| {
        ConfigError::invalid(format!("{message} in router ladder entry '{entry}'"))
    })?;
    let tier = ladder_tier_for_model(model, llm).ok_or_else(|| {
        ConfigError::invalid(format!(
            "router ladder model '{model}' must match llm.model_fast, llm.model_mid, or llm.model_strong"
        ))
    })?;
    Ok(RouterLadderStep {
        model: model.to_string(),
        tier,
        effort,
    })
}

fn ladder_tier_for_model(model: &str, llm: &LlmConfig) -> Option<Tier> {
    if model == llm.model_fast {
        Some(Tier::Fast)
    } else if model == llm.model_mid {
        Some(Tier::Mid)
    } else if model == llm.model_strong {
        Some(Tier::Strong)
    } else {
        None
    }
}

fn validate_router_ladder(steps: &[RouterLadderStep]) -> Result<(), ConfigError> {
    if steps.is_empty() {
        return Err(ConfigError::invalid("router ladder must not be empty"));
    }
    for window in steps.windows(2) {
        let prev = &window[0];
        let next = &window[1];
        if tier_rank(next.tier) < tier_rank(prev.tier) {
            return Err(ConfigError::invalid(format!(
                "router ladder must not downgrade from {:?} to {:?}",
                prev.tier, next.tier
            )));
        }
        if next.tier == prev.tier && effort_rank(next.effort) < effort_rank(prev.effort) {
            return Err(ConfigError::invalid(
                "router ladder must not decrease effort within the same model tier",
            ));
        }
    }
    Ok(())
}

fn tier_rank(tier: Tier) -> u8 {
    match tier {
        Tier::Fast => 0,
        Tier::Mid => 1,
        Tier::Strong => 2,
    }
}

fn effort_rank(effort: ReasoningEffort) -> u8 {
    match effort {
        ReasoningEffort::Low => 0,
        ReasoningEffort::Medium => 1,
        ReasoningEffort::High => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_file_and_env_and_cli_precedence() {
        let mut config = AppConfig::default();

        let file = FileConfig {
            agent: Some(FileAgentConfig {
                max_steps: Some(10),
                max_no_progress_steps: Some(12),
                memory: None,
            }),
            ..FileConfig::default()
        };
        file.apply(&mut config).expect("file apply");

        let env = EnvOverrides::from_pairs(vec![
            ("MBUS_MAX_STEPS".to_string(), "20".to_string()),
            ("MBUS_MAX_NO_PROGRESS_STEPS".to_string(), "22".to_string()),
        ])
        .expect("env overrides");
        env.apply(&mut config).expect("env apply");

        let cli = CliOverrides {
            max_steps: Some(30),
            max_no_progress_steps: Some(32),
            ..CliOverrides::default()
        };
        cli.apply(&mut config).expect("cli apply");

        assert_eq!(config.agent.max_steps, 30);
        assert_eq!(config.agent.max_no_progress_steps, 32);
    }

    #[test]
    fn maps_headless_to_headful() {
        let mut config = AppConfig::default();
        let file = FileConfig {
            browser: Some(FileBrowserConfig {
                headless: Some(true),
                ..FileBrowserConfig::default()
            }),
            ..FileConfig::default()
        };
        file.apply(&mut config).expect("file apply");
        assert!(!config.browser.headful);
    }

    #[test]
    fn parses_llm_mode_from_env() {
        let env =
            EnvOverrides::from_pairs(vec![("MBUS_LLM_MODE".to_string(), "openai".to_string())])
                .expect("env overrides");
        assert_eq!(env.inner.llm_mode, Some(LlmMode::OpenAi));
    }

    #[test]
    fn parses_llm_mode_with_case_and_whitespace() {
        assert_eq!(
            LlmMode::from_str(" OpenAI ").expect("parse openai"),
            LlmMode::OpenAi
        );
        assert_eq!(
            LlmMode::from_str("SCRIPTED").expect("parse scripted"),
            LlmMode::Scripted
        );
        assert_eq!(
            LlmMode::from_str("stub").expect("parse stub"),
            LlmMode::Stub
        );
    }

    #[test]
    fn rejects_unknown_llm_mode() {
        let err = LlmMode::from_str("gptz").expect_err("expected error");
        assert_eq!(err.to_string(), "unknown llm mode 'gptz'");
    }

    #[test]
    fn file_config_updates_router() {
        let mut config = AppConfig::default();
        let file = FileConfig {
            router: Some(FileRouterConfig {
                failures_to_mid: Some(5),
                failures_to_strong: Some(7),
                no_progress_to_mid: Some(3),
                no_progress_to_strong: Some(6),
                reasoning_effort: Some("high".to_string()),
                ..FileRouterConfig::default()
            }),
            ..FileConfig::default()
        };
        file.apply(&mut config).expect("file apply");
        assert_eq!(config.router.failures_to_mid, 5);
        assert_eq!(config.router.failures_to_strong, 7);
        assert_eq!(config.router.no_progress_to_mid, 3);
        assert_eq!(config.router.no_progress_to_strong, 6);
        assert_eq!(
            config.router.reasoning_effort,
            crate::types::ReasoningEffort::High
        );
    }

    #[test]
    fn rejects_empty_router_ladder() {
        let mut config = AppConfig::default();
        config.router.ladder_spec = Some(Vec::new());
        let err = normalize_router_ladder(&mut config).expect_err("expected error");
        assert_eq!(err.to_string(), "router ladder must not be empty");
    }

    #[test]
    fn rejects_router_ladder_downgrade() {
        let mut config = AppConfig::default();
        config.router.ladder_spec = Some(vec![
            format!("{}:medium", config.llm.model_mid),
            format!("{}:medium", config.llm.model_fast),
        ]);
        let err = normalize_router_ladder(&mut config).expect_err("expected error");
        assert!(err.to_string().contains("must not downgrade"));
    }

    #[test]
    fn browser_launch_config_respects_precedence() {
        let mut config = AppConfig::default();
        let file = FileConfig {
            browser: Some(FileBrowserConfig {
                executable_path: Some("/file/chrome".to_string()),
                launch_timeout_ms: Some(11_000),
                no_sandbox: Some(false),
                extra_args: Some(vec!["--file-arg".to_string()]),
                keep_user_data_dir: Some(false),
                ..FileBrowserConfig::default()
            }),
            ..FileConfig::default()
        };
        file.apply(&mut config).expect("file apply");

        let env = EnvOverrides::from_pairs(vec![
            (
                "MBUS_BROWSER_EXECUTABLE".to_string(),
                "/env/chrome".to_string(),
            ),
            (
                "MBUS_BROWSER_LAUNCH_TIMEOUT_MS".to_string(),
                "22000".to_string(),
            ),
            ("MBUS_BROWSER_NO_SANDBOX".to_string(), "true".to_string()),
            (
                "MBUS_BROWSER_EXTRA_ARGS".to_string(),
                "--env-one,--env-two".to_string(),
            ),
            (
                "MBUS_BROWSER_KEEP_USER_DATA_DIR".to_string(),
                "true".to_string(),
            ),
        ])
        .expect("env overrides");
        env.apply(&mut config).expect("env apply");

        let cli = CliOverrides {
            browser_executable: Some(PathBuf::from("/cli/chrome")),
            browser_launch_timeout_ms: Some(33_000),
            browser_no_sandbox: Some(false),
            browser_args: Some(vec!["--cli-arg".to_string()]),
            browser_keep_user_data_dir: Some(false),
            ..CliOverrides::default()
        };
        cli.apply(&mut config).expect("cli apply");

        assert_eq!(
            config.browser.executable_path,
            Some(PathBuf::from("/cli/chrome"))
        );
        assert_eq!(config.browser.launch_timeout, Duration::from_millis(33_000));
        assert!(!config.browser.no_sandbox);
        assert_eq!(config.browser.extra_args, vec!["--cli-arg".to_string()]);
        assert!(!config.browser.keep_user_data_dir);
    }

    #[test]
    fn parses_browser_extra_args_from_env_csv() {
        let env = EnvOverrides::from_pairs(vec![(
            "MBUS_BROWSER_EXTRA_ARGS".to_string(),
            "--alpha,--beta=1, --gamma ".to_string(),
        )])
        .expect("env overrides");
        assert_eq!(
            env.inner.browser_args,
            Some(vec![
                "--alpha".to_string(),
                "--beta=1".to_string(),
                "--gamma".to_string()
            ])
        );
    }

    #[test]
    fn parses_browser_extra_args_from_env_json_array() {
        let env = EnvOverrides::from_pairs(vec![(
            "MBUS_BROWSER_EXTRA_ARGS".to_string(),
            "[\"--alpha\",\"--beta=1\"]".to_string(),
        )])
        .expect("env overrides");
        assert_eq!(
            env.inner.browser_args,
            Some(vec!["--alpha".to_string(), "--beta=1".to_string()])
        );
    }

    #[test]
    fn parses_screenshot_persist_from_env() {
        let env = EnvOverrides::from_pairs(vec![(
            "MBUS_SCREENSHOT_PERSIST".to_string(),
            "on_error".to_string(),
        )])
        .expect("env overrides");
        assert_eq!(
            env.inner.screenshot_persist,
            Some(ScreenshotPersist::OnError)
        );
    }

    #[test]
    fn screenshot_config_respects_precedence() {
        let mut config = AppConfig::default();
        let file = FileConfig {
            screenshot: Some(FileScreenshotConfig {
                enabled: Some(true),
                persist: Some("always".to_string()),
            }),
            ..FileConfig::default()
        };
        file.apply(&mut config).expect("file apply");

        let env = EnvOverrides::from_pairs(vec![
            ("MBUS_SCREENSHOT_ENABLED".to_string(), "false".to_string()),
            (
                "MBUS_SCREENSHOT_PERSIST".to_string(),
                "on_error".to_string(),
            ),
        ])
        .expect("env overrides");
        env.apply(&mut config).expect("env apply");

        let cli = CliOverrides {
            screenshot_enabled: Some(true),
            screenshot_persist: Some(ScreenshotPersist::None),
            ..CliOverrides::default()
        };
        cli.apply(&mut config).expect("cli apply");

        assert!(config.screenshot.enabled);
        assert_eq!(config.screenshot.persist, ScreenshotPersist::None);
    }

    #[test]
    fn env_boolean_parsing_accepts_yes_no() {
        let env =
            EnvOverrides::from_pairs(vec![("MBUS_ALLOW_INSECURE".to_string(), "yes".to_string())])
                .expect("env overrides");
        assert_eq!(env.inner.allow_insecure, Some(true));
    }
}

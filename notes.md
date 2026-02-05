# Notes

## High-level architecture

A performant Rust “browser + LLM” agent usually ends up as a small state machine with strict interfaces:

* **Browser driver (CDP/WebDriver)**
  * Loads pages, queries DOM/accessibility tree, performs actions.
* **Observer**
  * Produces a compact `Observation` (URL, title, focused element, reduced element list with stable refs).
* **Policy + Router**
  * Chooses which model to call (fast by default, escalations).
  * Tracks failure streaks and “no progress” streaks.
* **Planner / Executor split**
  * Planner (strong model) produces a short plan and constraints.
  * Executor (fast model) emits single-step actions under a schema.
* **Verifier**
  * Deterministic checks first (action references exist, types match, page changed).
  * Optional tiny-model “repair” call when JSON parse or schema validation fails.
* **Memory**
  * Keeps the plan, last N observations, step history, and a progress hash.
* **Telemetry**
  * Tracing spans, counters, and per-step timing. Prometheus if you want dashboards.

## Rust crate choices (common)

* **Tokio** for async runtime.
* **CDP** for performance and richer page introspection.
  * `chromiumoxide` is a common Rust CDP client.
  * WebDriver alternatives: `fantoccini`, `thirtyfour` (often slower and less introspective).
* **Serde** for JSON and action parsing.
* **jsonschema** (or custom validation) for schema validation.
* **tracing`+`tracing-subscriber`** for logs.
* **prometheus`or`opentelemetry`** for metrics.

## Suggested module layout

```
src/
  main.rs
  agent/
    mod.rs
    loop.rs            // state machine
    policy.rs          // thresholds, timeouts
    memory.rs
  browser/
    mod.rs
    cdp.rs             // chromiumoxide adapter
    observe.rs         // DOM/AX snapshot -> Observation
    act.rs             // Action -> CDP operations
  llm/
    mod.rs
    client.rs          // trait + implementations
    router.rs          // fast->mid->strong escalation
    prompts.rs         // system prompts, few-shots
    schema.rs          // Action schema and validation
  verify/
    mod.rs
    rules.rs
    repair.rs
  types.rs             // Observation, ElementRef, Action, StepResult
```

## Core types (what matters)

```rust
// types.rs
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    pub url: String,
    pub title: String,
    pub viewport: (u32, u32),
    pub elements: Vec<ElementRef>,
    pub focused: Option<String>,      // element id
    pub visible_text: String,         // small, trimmed
    pub state_hash: String,           // for "no progress" detection
}

#[derive(Clone, Debug, Serialize)]
pub struct ElementRef {
    pub id: String,                   // stable ref used in actions
    pub role: String,                 // button, textbox, link, etc
    pub name: String,                 // accessible name/label
    pub value: Option<String>,
    pub bbox: Option<(f32, f32, f32, f32)>,
    pub flags: Vec<String>,           // disabled, readonly, etc
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Click { id: String },
    Type { id: String, text: String, submit: bool },
    Select { id: String, option: String },
    Scroll { dx: i32, dy: i32 },
    Wait { ms: u64 },
    Navigate { url: String },
    Back,
    Extract { query: String },
    Done { summary: String },
}

#[derive(Clone, Debug)]
pub struct StepResult {
    pub action: Action,
    pub ok: bool,
    pub error: Option<String>,
    pub new_state_hash: Option<String>,
}
```

## Key traits

```rust
// browser/mod.rs
use crate::types::{Action, Observation, StepResult};

#[async_trait::async_trait]
pub trait Browser: Send + Sync {
    async fn snapshot(&self) -> anyhow::Result<Observation>;
    async fn apply(&self, action: &Action) -> anyhow::Result<StepResult>;
    async fn wait_for_stable(&self) -> anyhow::Result<()>;
}
```

```rust
// llm/client.rs
use crate::types::{Action, Observation};

#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    async fn propose_action(
        &self,
        task: &str,
        plan: Option<&str>,
        obs: &Observation,
        history: &[String],
    ) -> anyhow::Result<Action>;
}
```

## Model router with fallback policy

Core idea: one fast model tries first, then a mid model, then a strong model. Escalation triggers are explicit and cheap to compute.

```rust
// llm/router.rs
pub struct Router {
    pub fast: Box<dyn LlmClient>,
    pub mid: Box<dyn LlmClient>,
    pub strong: Box<dyn LlmClient>,
    pub failures: u32,
    pub no_progress: u32,
    pub last_hash: Option<String>,
}

impl Router {
    pub fn on_step(&mut self, prev_hash: &str, new_hash: &str, ok: bool) {
        if !ok {
            self.failures += 1;
        }
        if prev_hash == new_hash {
            self.no_progress += 1;
        } else {
            self.no_progress = 0;
        }
        self.last_hash = Some(new_hash.to_string());
    }

    pub fn tier(&self) -> Tier {
        if self.failures >= 6 || self.no_progress >= 3 {
            Tier::Strong
        } else if self.failures >= 3 {
            Tier::Mid
        } else {
            Tier::Fast
        }
    }
}

pub enum Tier { Fast, Mid, Strong }
```

## Agent loop (the “engine”)

This is the piece you optimize first. Keep it tight, predictable, instrumented.

```rust
// agent/loop.rs
use crate::browser::Browser;
use crate::llm::router::{Router, Tier};
use crate::types::{Action, StepResult};

pub struct Agent<B: Browser> {
    pub browser: B,
    pub router: Router,
    pub task: String,
    pub plan: Option<String>,
    pub history: Vec<String>,
    pub max_steps: usize,
}

impl<B: Browser> Agent<B> {
    pub async fn run(&mut self) -> anyhow::Result<Action> {
        // Optional: plan once with strong model
        // self.plan = Some(self.compute_plan().await?);

        let mut prev_hash = String::new();

        for _ in 0..self.max_steps {
            self.browser.wait_for_stable().await?;
            let obs = self.browser.snapshot().await?;
            prev_hash = obs.state_hash.clone();

            let action = match self.router.tier() {
                Tier::Fast => self.router.fast.propose_action(&self.task, self.plan.as_deref(), &obs, &self.history).await?,
                Tier::Mid => self.router.mid.propose_action(&self.task, self.plan.as_deref(), &obs, &self.history).await?,
                Tier::Strong => self.router.strong.propose_action(&self.task, self.plan.as_deref(), &obs, &self.history).await?,
            };

            // Verify before executing
            crate::verify::rules::validate_action(&action, &obs)?;

            if matches!(action, Action::Done { .. }) {
                return Ok(action);
            }

            let step: StepResult = self.browser.apply(&action).await?;
            self.history.push(format!("{:?}", step.action));

            let new_hash = step.new_state_hash.clone().unwrap_or_else(|| prev_hash.clone());
            self.router.on_step(&prev_hash, &new_hash, step.ok);

            if !step.ok {
                // Optional: schema repair attempt before escalating
                // self.history.push(format!("error: {}", step.error.unwrap_or_default()));
            }
        }

        Ok(Action::Done { summary: "max_steps reached".into() })
    }
}
```

## Where “performance” actually comes from in code

* **Observation minimization**
  * Generate stable element refs and keep the element list small.
  * Prefer accessibility roles + names over raw HTML.
* **Strict action schema**
  * LLM output is always one action, always parseable.
  * Reject invalid actions without a browser round-trip.
* **Progress detection**
  * Hash a small canonical page signature (URL + title + top N element ids/names).
  * Escalate on “same hash” streaks.
* **Tight timeouts**
  * Timebox `snapshot`, `LLM call`, `apply(action)`, `wait_for_stable`.
* **Cold-start control**
  * Keep the browser process alive.
  * Reuse sessions and contexts.
* **Telemetry**
  * Without per-step timings you will guess wrong about bottlenecks.

## What the CDP adapter must do

The CDP implementation is where most complexity lives:

* `snapshot()`
  * Pull accessibility tree and/or DOM subset.
  * Map nodes to stable `ElementRef.id` values.
  * Compute `state_hash`.
* `apply(action)`
  * Resolve `id` to node coordinates or backend node id.
  * Execute click/type/select with proper focus behavior.
  * Wait for navigation or DOM changes where relevant.
  * Return `StepResult` with updated hash.

Avoid implementing CAPTCHA solving or bot-evasion logic. Build for legitimate automation and internal workflows.

Start DOM-only with a strong accessibility-centric observation and add optional vision as a last-resort fallback, not as the default.

A simple escalation trigger set:
- no_progress_hash >= 2 and DOM still reports the same candidates
- repeated “element not clickable / intercepted / detached” errors
- “surface collapse” (very few actionable nodes, mostly a canvas)
- page is a PDF viewer, chart canvas, or remote desktop detected by heuristics

That preserves performance while keeping an exit hatch for the cases DOM-only will churn on.

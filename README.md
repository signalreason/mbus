# mbus

Rust browser + LLM agent for deterministic, single-step web automation.

## Overview

mbus runs a tight loop of snapshot -> propose -> validate -> apply. Actions are
strictly validated against the current observation before execution, and every
step is logged as JSON for traceability.

Key traits:
- Chromium CDP browser adapter (`chromiumoxide`)
- Strict action schema + validation
- Model router with fast -> mid -> strong escalation
- Structured JSON logs plus tracing + metrics

## Install

Prerequisites:
- Rust toolchain (stable)
- A Chromium/Chrome binary discoverable by `chromiumoxide`

Build:

```bash
cargo build
```

## Quickstart

Default (stub LLM, immediately returns `done` after snapshot):

```bash
cargo run -- run --task "open example.com"
```

OpenAI mode:

```bash
MBUS_LLM_MODE=openai MBUS_LLM_API_KEY=... \
  cargo run -- run --task "Find the shipping address" \
  --llm-model-fast gpt-5.2-codex \
  --llm-model-mid gpt-5.2 \
  --llm-model-strong gpt-5.1-codex-max
```

Scripted mode (feed actions from a file):

```bash
cargo run -- run --task "Click the button" \
  --llm-mode scripted \
  --llm-actions-file ./actions.jsonl
```

For a concise install + quickstart path (prerequisites, install steps, and the first successful run with validated commands), see `docs/quickstart.md`.

## CLI

`mbus run` flags (most common):
- `--task` or `--task-file`
- `--plan` or `--plan-file`
- `--config`
- `--headless`
- `--initial-url`
- `--max-steps`
- `--llm-mode` (`stub`, `scripted`, `openai`)
- `--llm-base-url`, `--llm-api-key`
- `--llm-model-fast`, `--llm-model-mid`, `--llm-model-strong`
- `--llm-timeout-ms`, `--llm-temperature`, `--llm-max-tokens`
- `--llm-actions-file`
- `--extract-output`

`mbus bench` flags:
- `--tasks-dir` (default: `harness/tasks`)
- `--report-path` (default: `target/bench/report.json`)
- `--config`
- `--headless`
- `--max-steps-per-task` (default: `40`)
- `--required-passes` (default: total tasks minus two)
- `--llm-mode` (`scripted`, `openai`)
- `--llm-base-url`, `--llm-api-key`
- `--llm-model-fast`, `--llm-model-mid`, `--llm-model-strong`
- `--llm-timeout-ms`, `--llm-temperature`, `--llm-max-tokens`

## Benchmark Harness

Run the local benchmark harness:

```bash
cargo run -- bench --llm-mode scripted
```

The command:
- Starts a local HTTP harness server on `127.0.0.1` with deterministic pages.
- Serves static harness pages from `harness/pages`.
- Loads task fixtures from `harness/tasks/*.json`.
- Executes each task with scripted actions in `scripted` mode.
- Executes each task autonomously in `openai` mode (requires `MBUS_LLM_API_KEY` or `--llm-api-key`).
- Writes the report to `target/bench/report.json`.
- Enforces a gate (`required_passes`, default 8 of 10 tasks).

Task fixture shape (example):

```json
{
  "id": "bench-task-01",
  "task": "Navigate to benchmark task 01 and confirm marker text.",
  "start_path": "/bench/start",
  "max_steps": 40,
  "actions": [
    {"type": "navigate", "url": "{{base_url}}/bench/task-01"},
    {"type": "done", "summary": "Reached benchmark task 01"}
  ],
  "expect": {
    "status": "done",
    "final_url_contains": "/bench/task-01",
    "final_visible_text_contains": "BENCH TASK 01"
  }
}
```

## Config

Config precedence is: defaults -> config file -> env (`MBUS_*`) -> CLI flags.
Config file lookup order is: `--config`, `MBUS_CONFIG`, `./mbus.toml`, `~/.mbus.toml`.

Sample `mbus.toml`:

```toml
[agent]
max_steps = 40

[agent.memory]
max_observations = 8
max_history = 100

[browser]
headless = true
# headful = true
initial_url = "about:blank"
snapshot_timeout_ms = 5000
action_timeout_ms = 10000
max_elements = 50
max_text_len = 4000

[router]
failures_to_mid = 2
failures_to_strong = 4
no_progress_to_mid = 2
no_progress_to_strong = 4

[validator]
allow_insecure = false
max_text_len = 2000
max_wait_ms = 30000
max_scroll = 2000

[llm]
mode = "stub"
base_url = "https://api.openai.com/v1"
api_key = ""
model_fast = "gpt-5.2-codex"
model_mid = "gpt-5.2"
model_strong = "gpt-5.1-codex-max"
timeout_ms = 30000
temperature = 0.0
max_tokens = 256
actions_file = "actions.jsonl"

[output]
extract_output = "mbus_extract.json"
```

To run with a visible browser window, set `headful = true` in the config or
pass `--headless false` on the CLI.

Environment variable overrides (full list):
- `MBUS_CONFIG`
- `MBUS_MAX_STEPS`
- `MBUS_MEMORY_MAX_OBSERVATIONS`
- `MBUS_MEMORY_MAX_HISTORY`
- `MBUS_HEADLESS`
- `MBUS_INITIAL_URL`
- `MBUS_CDP_URL`
- `MBUS_SNAPSHOT_TIMEOUT_MS`
- `MBUS_ACTION_TIMEOUT_MS`
- `MBUS_MAX_ELEMENTS`
- `MBUS_MAX_TEXT_LEN`
- `MBUS_ROUTER_FAILURES_TO_MID`
- `MBUS_ROUTER_FAILURES_TO_STRONG`
- `MBUS_ROUTER_NO_PROGRESS_TO_MID`
- `MBUS_ROUTER_NO_PROGRESS_TO_STRONG`
- `MBUS_ALLOW_INSECURE`
- `MBUS_VALIDATOR_MAX_TEXT_LEN`
- `MBUS_VALIDATOR_MAX_WAIT_MS`
- `MBUS_VALIDATOR_MAX_SCROLL`
- `MBUS_LLM_MODE`
- `MBUS_LLM_BASE_URL`
- `MBUS_LLM_API_KEY`
- `MBUS_LLM_MODEL_FAST`
- `MBUS_LLM_MODEL_MID`
- `MBUS_LLM_MODEL_STRONG`
- `MBUS_LLM_TIMEOUT_MS`
- `MBUS_LLM_TEMPERATURE`
- `MBUS_LLM_MAX_TOKENS`
- `MBUS_LLM_ACTIONS_FILE`
- `MBUS_EXTRACT_OUTPUT`

## Scripted Actions Format

Scripted actions accept any of the following formats:
- A JSON array of actions
- A single JSON action object
- JSON Lines (one action per line)

Example (`actions.jsonl`):

```json
{"type":"navigate","url":"https://example.com"}
{"type":"click","id":"el_1"}
{"type":"done","summary":"clicked"}
```

## Logs and Telemetry

- `mbus run` prints JSON log lines to stdout (`type = config | step | summary`).
- Tracing logs are emitted as JSON to stderr; set `RUST_LOG=info` or similar to
  control verbosity.
- Metrics are in-process counters and timers; see `src/telemetry.rs` for names.

## Troubleshooting

- Chromium fails to launch: install Chromium/Chrome and ensure it is
  discoverable by `chromiumoxide`.
- OpenAI 401/403: ensure `MBUS_LLM_API_KEY` is set for `openai` mode.
- Invalid scripted actions: confirm the JSON matches the action schema and
  references real element ids.
- Timeouts on slow pages: increase `snapshot_timeout_ms` or `action_timeout_ms`.
- Navigation to non-http(s) URLs blocked: set `allow_insecure = true` only when
  needed and understand the security implications.

For a structured operations runbook, recovery steps, and the log/metric fields
you should monitor, see `docs/operations-runbook.md`.

## Runbook

Verification:
- `cargo test`
- Run a short task with `mbus run` and confirm a `summary` JSON log line is
  emitted and, if using extract actions, `mbus_extract.json` is written.

Rollback:
- Checkout the previous release tag or commit and rebuild.
- Revert any config changes (especially router thresholds and timeouts) to the
  last known-good values.

For the full verification checklist, rollback recipe, and structured logging
guidance, see `docs/operations-runbook.md`.

# Product Requirements Document: mbus (Rust browser + LLM agent)

## Executive Summary
- Build a Rust-based browser automation agent that uses LLMs to choose single-step actions through a strict JSON schema.
- Target internal automation and evaluation workflows that need fast, deterministic, and observable browser steps.
- Provide a pluggable browser backend (CDP) and pluggable LLM clients with explicit model escalation.
- Emphasize performance and telemetry so bottlenecks are measurable rather than guessed.
- Ship a CLI-driven MVP first, then harden with repair and progress heuristics.
Success definition: On a 10-task internal web harness, the agent completes at least 8 tasks within 40 steps each, and produces structured logs + metrics for every run.

## Scope
### In scope:
- Core state-machine agent loop with max-step limits and deterministic termination.
- CDP-based browser adapter (Chromium via chromiumoxide) with snapshot + action apply.
- Observation model with stable element refs and compact visible text.
- Strict action schema, validation, and error handling.
- LLM router with fast -> mid -> strong escalation based on failure/no-progress streaks.
- Memory of plan, last N observations, and action history.
- CLI entrypoint and config file support.
- Tracing and metrics for per-step timing and failures.
### Out of scope:
- CAPTCHA solving, bot evasion, or adversarial browsing behavior.
- Vision-first browsing or heavy OCR pipelines.
- Multi-agent coordination or distributed execution.
- UI dashboard for managing runs.
### Non-goals:
- Maximizing model autonomy at the expense of determinism.
- Persisting or indexing raw page content by default.
- Supporting every browser engine; Chromium-only for MVP.

## Requirements
### Functional requirements:
1. Accept a task string and optional plan; execute up to `max_steps` and return a final summary or error.
2. Launch or connect to a headless Chromium instance via CDP and keep the session warm.
3. Produce an `Observation` with URL, title, viewport, focused element, compact visible text, element list, and a stable `state_hash`.
4. Generate exactly one `Action` per step from an LLM client via the JSON schema.
5. Validate actions deterministically against the current observation before execution.
6. Apply actions (click/type/select/scroll/wait/navigate/back/extract/done) and return `StepResult` including `ok`, `error`, and `new_state_hash` when available.
7. Escalate models based on failure and no-progress counters; reset counters on progress.
8. Maintain action history and recent observations for context and debugging.
9. Stop on `Done` action, or on `max_steps`, and return a machine-readable final record.
10. Provide a CLI that outputs step-by-step JSON logs and final summary.
### Non-functional requirements:
- Performance: Observation and action execution should each complete within 1 second on typical pages (excluding LLM latency).
- Reliability: No panics in normal operation; all errors returned as structured results.
- Security: Do not log secrets or form input by default; restrict navigation to http/https unless explicitly allowed.
- Observability: Structured logs, traces, and metrics for every step and LLM call.
- Maintainability: Module boundaries follow the documented layout with clear traits.
- Portability: macOS and Linux support for MVP.
### Data requirements:
- Entities: Observation, ElementRef, Action, StepResult, Plan, History, Config, Telemetry events.
- Retention: In-memory by default; optional file logging configurable by the operator.
- Privacy: Treat page content as sensitive; do not persist raw DOM or screenshots without explicit opt-in.
### Integration requirements:
- Browser: Chromium via CDP (chromiumoxide).
- LLM: OpenAI-compatible HTTP API client; pluggable trait for other providers.
- Metrics: Prometheus or OpenTelemetry exporter (optional but supported).
### Edge cases / failure modes:
1. Element id not found or detached between snapshot and action.
2. Navigation or page reload during action execution.
3. LLM returns invalid JSON or an unknown action type.
4. `state_hash` unchanged across multiple steps (no-progress loop).
5. Page with very few actionable nodes (canvas, PDF viewer).
6. Form fields requiring focus or special input types.
7. Slow network or long-running navigation timeouts.

## UX / API Contract
User-facing flow (CLI):
- `mbus run --task "..." [--max-steps 40] [--headless true] [--config mbus.toml]`
- States: initializing -> running -> done | failed | max_steps
- Output: JSON lines per step, plus a final summary record.
Copy notes:
- Errors should include a short user-facing reason and a machine-readable code.

Action schema (JSON example):
```
{ "type": "click", "id": "el_42" }
{ "type": "type", "id": "el_7", "text": "search term", "submit": true }
{ "type": "done", "summary": "Found the shipping address" }
```

Observation schema (JSON example):
```
{
  "url": "https://example.com",
  "title": "Checkout",
  "viewport": [1280, 800],
  "focused": "el_7",
  "visible_text": "Cart Total $19.99 ...",
  "state_hash": "ab12cd",
  "elements": [
    { "id": "el_7", "role": "textbox", "name": "Email", "value": null, "bbox": [10, 120, 400, 36], "flags": [] }
  ]
}
```

Validation rules and invariants:
- `Action.id` must exist in `Observation.elements` when required by action type.
- `Navigate.url` must be http/https unless `allow_insecure` is true.
- `Type.text` length <= 2000 characters.
- `Wait.ms` <= 30000.
- `Scroll.dx/dy` are bounded to +/- 2000 per action.

## Observability + Operations
### Metrics:
- `step_duration_ms`, `snapshot_duration_ms`, `apply_duration_ms`
- `llm_duration_ms`, `llm_failures_total`
- `actions_total{type}`, `steps_total`, `no_progress_streak`
### Logs and traces:
- Structured JSON logs per step, including action, outcome, and timings.
- Trace spans for snapshot, LLM call, validation, action apply.
### SLOs / SLIs:
- SLI: % of steps that complete successfully without retry.
- Target: 95% successful step execution on the harness.
### Runbook notes:
- Verify by running the CLI on a known demo site and checking metrics output.
- Rollback by pinning to the previous release tag and disabling new config flags.

## Security + Compliance
### Threat model:
- Prompt injection from untrusted pages.
- Data exfiltration via LLM output or logs.
- Malicious pages causing runaway navigation or resource usage.
### Data classification and access control:
- Treat all observed page content as confidential by default.
- API keys loaded from environment or secret manager; never logged.
### Secrets management expectations:
- Support `MBUS_LLM_API_KEY` env var and allow external secret injection.
### Audit needs:
- Record high-level metadata (task id, timestamps, model tier) without raw page content.

## Release Plan (Milestones)
### Milestone 0: Discovery / spikes
- Validate CDP snapshot and action application on a real page.
- Validate LLM JSON schema round-trip and error handling.
#### Acceptance criteria:
- A script can open a page, list actionable elements, and click one.
- A sample action JSON passes schema validation and can be parsed.
#### Demo checklist:
- Show a single click action on example.com.
- Show schema validation rejecting a malformed action.

### Milestone 1: MVP
- Implement core types, schema, browser adapter, validator, router, agent loop, CLI, and telemetry.
- Run on a small harness of pages and produce structured logs.
#### Acceptance criteria:
- `mbus run` completes at least 6/10 harness tasks within 40 steps.
- Every step logs action + outcome and emits metrics.
- Invalid actions are rejected before browser execution.
#### Demo checklist:
- Run a task end-to-end from CLI and show final summary.
- Show logs and metrics output for a run.

### Milestone 2: Enhancements
- Add schema repair on invalid LLM output and stronger no-progress heuristics.
- Add structured extraction outputs and improved docs.
#### Acceptance criteria:
- JSON repair handles at least 50% of malformed outputs in a test set.
- No-progress detection escalates models on repeated hash matches.
#### Demo checklist:
- Show a run where a malformed action is repaired and execution continues.
- Show extraction of a value into a structured output.

### Milestone 3: Autonomous benchmark proof
- Validate harness performance with autonomous model decisions, not scripted fixture playback.
- Add benchmark reporting for LLM token usage and estimated token cost.
- Preserve deterministic benchmark gating while adding an autonomous execution mode.
#### Acceptance criteria:
- `mbus bench` supports `scripted` and `openai` modes through config/CLI, with no hardcoded scripted override.
- In `openai` mode, benchmark report includes per-task and aggregate token usage (`prompt_tokens`, `completion_tokens`, `total_tokens`).
- Report includes aggregate USD cost using configurable per-1M-token pricing for input and output tokens.
- Benchmark gate remains enforced (`required_passes`, step limit) and is reported for both modes.
- A reproducible run artifact captures mode, model names, timing, gate result, and token/cost totals.
#### Demo checklist:
- Run `mbus bench` in `openai` mode on the 10-task harness and produce a report artifact.
- Show gate result plus aggregate duration, token totals, and estimated cost.
- Run `mbus bench` in `scripted` mode to confirm backward compatibility.

## Task List for Engineering (engineer-ready)

### Milestone M0

1. [Spike] CDP snapshot feasibility

| Field                | Value                                                                                      |
| -------------------- | ------------------------------------------------------------------------------------------ |
| Goal/Rationale       | Confirm chromiumoxide can produce a compact observation and click an element reliably.     |
| Implementation notes | Build a small harness that opens a page, dumps roles/names, and clicks by backend node id. |
| Dependencies         | None                                                                                       |
| Acceptance criteria  | Can list at least 10 elements and click one on a demo site without crash.                  |
| Test notes           | Manual run against example.com and a simple form page.                                     |
| Observability notes  | Log timings for snapshot and click.                                                        |

2. [Spike] LLM schema round-trip

| Field                | Value                                                                      |
| -------------------- | -------------------------------------------------------------------------- |
| Goal/Rationale       | Ensure action JSON parsing/validation works before wiring full agent loop. |
| Implementation notes | Define schema and parse sample actions; reject malformed input.            |
| Dependencies         | None                                                                       |
| Acceptance criteria  | Valid actions parse; invalid ones fail with clear error.                   |
| Test notes           | Unit tests for parsing and validation.                                     |
| Observability notes  | Log parse failures with error codes.                                       |

### Milestone M1

3. [Build] Crate scaffold and module layout

| Field                | Value                                                          |
| -------------------- | -------------------------------------------------------------- |
| Goal/Rationale       | Establish the project structure aligned to the architecture.   |
| Implementation notes | Create crate, modules, and placeholder traits.                 |
| Dependencies         | None                                                           |
| Acceptance criteria  | Project builds; module skeleton matches the documented layout. |
| Test notes           | `cargo test` runs with a placeholder test.                     |
| Observability notes  | None.                                                          |

4. [Build] Core types and schema

| Field                | Value                                                                 |
| -------------------- | --------------------------------------------------------------------- |
| Goal/Rationale       | Provide shared types for Observation/Action/StepResult.               |
| Implementation notes | Define structs/enums with serde derive and schema validation helpers. |
| Dependencies         | Task 3                                                                |
| Acceptance criteria  | Types compile and serialize; schema validates examples.               |
| Test notes           | Unit tests for serialization and schema validation.                   |
| Observability notes  | None.                                                                 |

5. [Build] Browser adapter (CDP)

| Field                | Value                                                                       |
| -------------------- | --------------------------------------------------------------------------- |
| Goal/Rationale       | Provide snapshot and apply(action) backed by chromiumoxide.                 |
| Implementation notes | Implement Browser trait; manage lifecycle and timeouts.                     |
| Dependencies         | Tasks 1, 3, 4                                                               |
| Acceptance criteria  | Snapshot returns valid Observation; click/type actions work on a demo page. |
| Test notes           | Integration test against a local static page.                               |
| Observability notes  | Emit timings for snapshot/apply.                                            |

6. [Build] Observation builder + state hash

| Field                | Value                                                                    |
| -------------------- | ------------------------------------------------------------------------ |
| Goal/Rationale       | Provide compact, stable element refs and progress detection.             |
| Implementation notes | Use accessibility roles/names; compute hash from URL/title/top elements. |
| Dependencies         | Tasks 4, 5                                                               |
| Acceptance criteria  | Element ids remain stable across snapshots; hash changes on navigation.  |
| Test notes           | Unit tests for hash determinism.                                         |
| Observability notes  | Log hash changes per step.                                               |

7. [Build] Action validator

| Field                | Value                                                |
| -------------------- | ---------------------------------------------------- |
| Goal/Rationale       | Reject invalid actions before browser execution.     |
| Implementation notes | Check id existence, bounds, and type constraints.    |
| Dependencies         | Task 4                                               |
| Acceptance criteria  | Invalid actions are rejected with structured errors. |
| Test notes           | Unit tests for all action types.                     |
| Observability notes  | Count validation failures.                           |

8. [Build] Model router

| Field                | Value                                                     |
| -------------------- | --------------------------------------------------------- |
| Goal/Rationale       | Escalate model tiers based on failures/no-progress.       |
| Implementation notes | Track counters and choose tier; expose config thresholds. |
| Dependencies         | Tasks 4, 2                                                |
| Acceptance criteria  | Tier changes based on counters; resets on progress.       |
| Test notes           | Unit tests for tier logic.                                |
| Observability notes  | Log tier changes.                                         |

9. [Build] Agent loop

| Field                | Value                                                            |
| -------------------- | ---------------------------------------------------------------- |
| Goal/Rationale       | Orchestrate snapshot -> propose -> validate -> apply -> record.  |
| Implementation notes | Implement max-step loop with history and termination conditions. |
| Dependencies         | Tasks 5, 6, 7, 8                                                 |
| Acceptance criteria  | A simple task completes and returns Done within max steps.       |
| Test notes           | Integration test with a local page.                              |
| Observability notes  | Per-step spans and timing.                                       |

10. [Build] CLI + config

| Field                | Value                                               |
| -------------------- | --------------------------------------------------- |
| Goal/Rationale       | Provide a usable entrypoint for operators.          |
| Implementation notes | Use clap; allow config file overrides and env vars. |
| Dependencies         | Task 9                                              |
| Acceptance criteria  | `mbus run` executes a task and prints JSON logs.    |
| Test notes           | CLI smoke test in CI.                               |
| Observability notes  | Log config values at startup (without secrets).     |

11. [Build] Telemetry (logs + metrics)

| Field                | Value                                                         |
| -------------------- | ------------------------------------------------------------- |
| Goal/Rationale       | Make performance and failures visible.                        |
| Implementation notes | Use tracing; optional Prometheus/OTel exporter.               |
| Dependencies         | Task 9                                                        |
| Acceptance criteria  | Metrics emitted for steps and LLM calls; logs are structured. |
| Test notes           | Unit test that metrics counters increment.                    |
| Observability notes  | Self-observing metrics only.                                  |

12. [Build] Test harness (local pages)

| Field                | Value                                               |
| -------------------- | --------------------------------------------------- |
| Goal/Rationale       | Provide repeatable tests for actions.               |
| Implementation notes | Serve simple HTML pages and run integration tests.  |
| Dependencies         | Tasks 5, 10                                         |
| Acceptance criteria  | Tests run and validate click/type/select behaviors. |
| Test notes           | Integration tests only.                             |
| Observability notes  | Log test timings.                                   |

### Milestone M2

13. [Build] Schema repair on invalid output

| Field                | Value                                                     |
| -------------------- | --------------------------------------------------------- |
| Goal/Rationale       | Recover from malformed LLM responses.                     |
| Implementation notes | Add small repair step before escalation.                  |
| Dependencies         | Tasks 7, 8                                                |
| Acceptance criteria  | A set of malformed samples are repaired to valid actions. |
| Test notes           | Unit tests for repair behavior.                           |
| Observability notes  | Count repairs vs failures.                                |

14. [Build] Improved no-progress heuristics

| Field                | Value                                                         |
| -------------------- | ------------------------------------------------------------- |
| Goal/Rationale       | Avoid loops on static pages.                                  |
| Implementation notes | Add heuristics for unchanged DOM and low-actionability pages. |
| Dependencies         | Tasks 6, 8                                                    |
| Acceptance criteria  | Escalation occurs after configured no-progress threshold.     |
| Test notes           | Unit tests for heuristic triggers.                            |
| Observability notes  | Log no-progress streaks.                                      |

15. [Build] Structured extraction output

| Field                | Value                                                     |
| -------------------- | --------------------------------------------------------- |
| Goal/Rationale       | Enable data capture tasks.                                |
| Implementation notes | Add `Extract` action results to a structured output file. |
| Dependencies         | Tasks 4, 9                                                |
| Acceptance criteria  | Extracted data is written to a JSON file for a demo page. |
| Test notes           | Integration test for extract output.                      |
| Observability notes  | Log extract size and path.                                |

16. [Docs] Usage and operations guide

| Field                | Value                                                  |
| -------------------- | ------------------------------------------------------ |
| Goal/Rationale       | Make it easy to run and troubleshoot.                  |
| Implementation notes | Document setup, config, common errors, and runbook.    |
| Dependencies         | Tasks 10, 11                                           |
| Acceptance criteria  | Docs cover install, run, and troubleshooting sections. |
| Test notes           | Doc review only.                                       |
| Observability notes  | None.                                                  |

### Milestone M3

17. [Build] Autonomous benchmark mode + cost telemetry

| Field                | Value                                                                                                                                |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Goal/Rationale       | Close the final validation gap by proving harness success under autonomous LLM decisions and publishing operational run economics.  |
| Implementation notes | Remove scripted-only forcing in bench path; add bench LLM mode selection (`scripted` or `openai`), usage/cost aggregation, and report schema updates. |
| Dependencies         | Tasks 9, 10, 11, 12                                                                                                                  |
| Acceptance criteria  | `mbus bench --llm-mode openai` runs end-to-end, enforces existing gate, and writes a report containing pass/fail, duration, token totals, and cost totals. |
| Test notes           | Unit tests for report aggregation math and config parsing; integration benchmark test in scripted mode plus mocked-usage openai path. |
| Observability notes  | Emit benchmark-level metrics/log fields for usage and cost, with explicit currency and pricing config used for the run.             |

## Open Questions / Assumptions
### Questions blocking execution:
- Which LLM provider(s) and model names are approved for MVP? -- OpenAI; gpt-5.2-codex, gpt-5.2, gpt-5.1-codex-max, gpt-5.1-codex-mini.
- Is a headless Chromium binary available in the target runtime environment? -- Include a script to install, or at least instructions.
### Questions that can wait:
- Should we support vision-based fallback in M2 or later? -- later
- Do we need multi-tab or multi-session support? -- not yet.
### Assumptions made (with rationale):
- We can use an OpenAI-compatible HTTP API for the initial LLM client to keep scope small. -- ok.
- Headless Chromium is acceptable for MVP; no need for full browser UI. -- acceptable, but prefer an option to view browser activity in dev mode.
- Logs and metrics will be collected via standard stdout scraping or Prometheus. -- ok.

## Risks + Tradeoffs
- Risk: CDP integration instability on dynamic sites. Severity: High. Mitigation: Start with limited harness and robust timeouts.
- Risk: Prompt injection causing unsafe actions. Severity: High. Mitigation: Strict schema validation and navigation allowlist.
- Risk: LLM latency dominates step time. Severity: Medium. Mitigation: Fast model default and tier escalation.
- Risk: Observation too large or unstable. Severity: Medium. Mitigation: Aggressive minimization and stable ids.
- Tradeoff: Speed vs correctness, favoring fast models with escalation.
- Tradeoff: Scope vs polish, keeping MVP minimal to ship quickly.

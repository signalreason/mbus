# Operations Runbook

## Purpose
Deliver a single place for operators to understand mbus observability, recover from recurring failure modes, and follow a verified change/rollback recipe. The troubleshooting sections reference the structured log events and error codes that appear on stdout or stderr, which avoids guesswork when parsing JSON traces.

The primary release-proof workflow is `mbus challenge` on the 12-task local obstacle suite, followed by `mbus package`. The 10-task bench remains useful regression coverage but is not the product-level outcome gate.

## Observability

### Key structured log events
| Event | Key fields | Notes |
| --- | --- | --- |
| `step_result` | `outcome`, `ok`, `error_code`, `tier`, `apply_duration_ms`, `snapshot_duration_ms`, `step_duration_ms`, `state_hash_unchanged`, `state_hash_streak`, `actionability_score`, `low_actionability`, `actionables_unchanged`, `too_few_actionables`, `prev_actionables`, `next_actionables` | Every successful step emits this event; use `error_code != "none"` to highlight failures and `state_hash_*` fields to track progress loops. |
| `llm_error` / `llm_failure` | `error_code`, `error_message_len`, `step_duration_ms` | Logged when the pre‑execution call to the LLM client (fast/mid/strong) fails. `telemetry::inc_llm_failure` also emits a `metric` event with `metric_name="llm_failures_total"` and `error_code`. |
| `validation_failed` | `error_count`, `tier`, `step_duration_ms` | Validation rejections log the number of `ValidationError`s encountered before the action is applied. |
| `apply_error` | `error_code`, `error_message`, `step_duration_ms` | Occurs when `Browser::apply` returns a `BrowserError`. |
| `snapshot_error` | `error_code`, `error_message`, `step_duration_ms` | Snapshot failures during the next observation capture. |
| `no_progress_termination` | `state_hash_streak`, `max_no_progress_steps` | Firewall for stuck runs. |
| `repair_success`, `repair_failed` | `error_code`, `repair_error` | Repair attempts triggered by malformed LLM output. Metrics emit `metric_name="repair_attempts_total"`/`"repair_failures_total"` and pair with `error_code`. |

### Metrics to monitor
- `metric_name=llm_failures_total` (`value=1`, `error_code` attached) — correlates with high `llm_error` volume.  
- `metric_name=repair_failures_total` / `repair_attempts_total` — repeated repair failures can mean the schema repair heuristics can no longer make sense of the LLM response (look for `repair_error`).  
- `metric_name=no_progress_streak` (set via `telemetry::set_no_progress_streak`) — pair with `step_result.state_hash_streak` to detect loops.  
- `step_duration_ms`, `apply_duration_ms`, `snapshot_duration_ms`, `llm_duration_ms` metrics (`metric` events emitted from `telemetry::record_*`) to identify regressions in each stage.  
- `actions_*_total` counters show workload mix. Use `RUST_LOG=json` + `jq` or your log aggregator to roll them up per run.

## Troubleshooting
Every section below ties failure symptoms to structured log events, error codes defined in the codebase, and concrete recovery steps.

### 1. LLM proposal failures (`llm_error` / `llm_failure`)
**Log signals:**  
  * `llm_error` event with `error_code` and `step_duration_ms`.  
  * `llm_failure` warning (same `error_code` with message length).  
  * `metric` events for `llm_failures_total`/`repair_failures_total` include the same `error_code`.  
**Common `error_code`s:** `missing_api_key`, `client_error`, `serialize_error`, `invalid_json`, `schema_violation`, `deserialize_error`, `multi_action`, `http_error`, `empty_response`, `timeout`, `transport_error`.  
**Recovery:**  
  1. Ensure `MBUS_LLM_API_KEY` (or `llm.api_key` in `mbus.toml`) is correct and not expired.  
  2. If `timeout`/`transport_error` persists, verify network connectivity and throttle reuse; consider increasing `llm.timeout_ms`.  
  3. JSON parsing codes (`invalid_json`, `schema_violation`, `multi_action`) mean the model produced malformed output. Drop the run into `--llm-mode scripted` to reproduce the raw response, inspect the trace via `tracing_subscriber` (set `RUST_LOG=trace`), and, if necessary, tighten the prompt or stash a shorter `plan`.  
  4. Watch `repair_failed` events and `repair_error` text—if repair fails consistently, the schema may need updates or the model choice should change (switch to `gpt-5.2` via `--llm-model-strong` or use `--llm-mode stub` for testing).  
  5. After adjusting config, rerun the verification checklist below.

### 2. Validation rejects (`validation_failed`, `step_result.error_code`)
**Log signals:**  
  * `validation_failed` event shows `error_count`.  
  * The follow-up `step_result` has `error_code="invalid_action"` and `step_result.outcome` stays `Failure`.  
  * `StepResult.error.validation_code` (exposed via in-memory records) mirrors the first `ValidationError`.  
**Validation `code`s:** `missing_id`, `unknown_id`, `text_too_long`, `scroll_out_of_bounds`, `wait_too_long`, `missing_url`, `invalid_url`, `insecure_url`, `repeat_no_progress_action`.  
**Recovery:**  
  1. Confirm the observation still contains the element referenced by the action (look at `Observation.elements`). If the page changed, adjust the task or plan to match the new DOM.  
  2. For `text_too_long`, `wait_too_long`, or `scroll_out_of_bounds`, raise the relevant caps in `[validator]` (see `mbus.toml`) or break the work into smaller actions.  
  3. `invalid_url`/`insecure_url` typically mean the model is trying to navigate to a blocked scheme—either allow it via `validator.allow_insecure = true` (after assessing the risk) or constrain the prompt to http(s).  
  4. `repeat_no_progress_action` means the same action was already attempted in the current unchanged state hash. Change action type/target before retrying.
  5. Re-run the offending step with `--plan` or `--task-file` to replicate the action history; once the validation codes stop firing, proceed with the run.

### 3. Browser-level failures (`apply_error`, `snapshot_error`)
**Log signals:** `apply_error`/`snapshot_error` events with `error_code`, `error_message`, `step_duration_ms`. Subsequent `step_result.error_code` inherits the same `BrowserError`.  
**Browser `error_code`s:** `config_error`, `cdp_launch_failed`, `cdp_connect_failed`, `cdp_page_close_failed`, `cdp_close_failed`, `cdp_handler_failed`, `cdp_page_failed`, `cdp_nav_failed`, `cdp_error`, `missing_url`, `js_error`.  
**Current status:** Browser startup stability is now an active delivery blocker, not just a local troubleshooting concern. In the current Codex environment, browser-backed `bench` and `challenge` validation fail before step execution begins because Chromium exits during startup with `cdp_launch_failed`.  
**Recovery:**  
  1. Check that the Chromium binary is accessible (see `chromiumoxide` requirements) and that no other process is holding the CDP port. Ensure `MBUS_HEADLESS`/`headful` config points to a valid binary.  
  2. For navigation failures (`cdp_nav_failed`), confirm the target URL is reachable and not blocked by certificates; if needed, set `validator.allow_insecure = true` or pass `--initial-url` to seed a safe start page.  
  3. `js_error` from observation collection usually means the page script threw while reading visible text; rerun the step with `RUST_LOG=debug` to see the exact JS stack, or increase `browser.max_text_len` if truncation fails.  
  4. Restart the run (`cargo run -- run ...`) after clearing state (delete `target/` if caching matters) and capture the first `snapshot_error` event to triage.

Until a dedicated browser preflight command exists, do not begin expensive `bench` or `challenge` proof runs unless browser startup has already been validated in the same environment. Treat `cdp_launch_failed` as a release-blocking runtime issue first, then return to proof generation.

### 4. No-progress / low-actionability loops (`step_result`, `no_progress_termination`)
**Log signals:**  
  * `step_result.actionability_score`, `state_hash_unchanged`, `state_hash_streak`, and `too_few_actionables` fields.  
  * `telemetry::set_no_progress_streak` raises `metric_name=no_progress_streak`.  
  * `no_progress_termination` warns when `state_hash_streak >= policy.max_no_progress_steps`.  
**Recovery:**  
  1. Inspect the actionable element counts from `step_result.prev_actionables` / `next_actionables`. If they stay equal and `actionables_unchanged=true`, the page lacks new inputs—consider modifying the task to navigate, inserting manual clicks (via `scripted actions`), or increasing `max_no_progress_steps`.  
  2. If `low_actionability=true`, add intentional navigation or waits to refresh the DOM.  
  3. When stuck, escalate the run manually (`MBUS_LLM_MODE=openai` with a stronger model) or restart the browser to force a fresh snapshot.  
  4. Use the `bench` harness to reproduce the loop in a deterministic fixture to test fixes.

### 5. Repair pipeline visibility (`repair_success` / `repair_failed`)
**Log signals:** `repair_success` indicates `repair_action` succeeded; `repair_failed` surfaces `error_code` plus `repair_error`.  
**Recovery:**  
  1. A string of `repair_failed` events with the same `error_code` means the schema is still rejecting model output. Try reducing prompt context or switching to a more conservative model (strong tier).  
  2. If repairs succeed but the run still stalls, the action may be structurally wrong; follow the validation troubleshooting above.  
  3. Use `mbus run --llm-mode stub` and feed known-good actions from `actions.jsonl` to confirm the browser path still works.

## Challenge Operations

### Canonical proof run
Use the helper script when generating product-level evidence:

```bash
MBUS_LLM_API_KEY=... \
MBUS_LLM_INPUT_COST_PER_MILLION=... \
MBUS_LLM_OUTPUT_COST_PER_MILLION=... \
./scripts/run_challenge_proof.sh
```

Expected outputs:
- a challenge report under `target/challenge/`,
- a packaged bundle directory under `target/challenge/package/`,
- a zip archive next to the unpacked bundle.

Prerequisite: confirm browser startup is healthy before running the proof script. The planned steady-state workflow includes a lightweight browser preflight check, but that check is not implemented yet.

### Challenge failure buckets
Challenge reports use the same aggregate structure as bench reports, but the most useful buckets are usually:
- `run_error:*`: browser launch, transport, timeout, or provider failures prevented evaluation.
- `status_mismatch:*`: the agent terminated without `done`.
- `visible_text_mismatch:*`: the page changed, but not to the expected observable success state.
- `final_url_mismatch:*`: navigation succeeded, but the final location is not the one declared by the manifest.
- `missing_screenshot_artifact`: artifact persistence broke even though the challenge path requires screenshots.
- `disallowed_final_url`: the run left the allowed domain set, which is treated as invalid even if the task otherwise looks successful.

When reviewing a proof package, start with `failure_buckets`, then inspect per-task `failure_reason`, screenshots, and final visible text.

## Runbook

### Verification checklist
1. `cargo test` – ensures core crates (telemetry, validator, browser adapters) still compile.  
2. Run a short scripted task: `cargo run --bin mbus -- run --llm-mode scripted --task "Checkout sample" --plan "" --max-steps 5` (or point at `harness/tasks/` fixture). Confirm the CLI emits a `summary` JSON line with `status="done"` and `final_url_contains`.  
3. Manually validate browser startup before any multi-task run. At minimum, confirm a browser-backed `mbus run` can launch Chromium cleanly in the current environment. If you hit `cdp_launch_failed`, stop and fix runtime setup before proceeding.  
4. Run `cargo run --bin mbus -- bench --llm-mode scripted` only after browser startup is healthy, to make sure the regression harness still meets its gate.  
5. For product-level evidence, run `./scripts/run_challenge_proof.sh` with the required env vars only after browser startup is healthy, and confirm the packaged report includes screenshots plus token and cost totals.  
6. If you rely on extraction outside the challenge flow, ensure `mbus_extract.json` is created and matches whichever `extract` action was used.  

### Rollback procedure
1. Identify the last known-good commit or tag (e.g., `git describe --tags --abbrev=0`).  
2. Check out that ref in a clean working tree: `git checkout <tag>` (do not force-reset shared branches).  
3. Restore any customized config (`mbus.toml`, environment overrides) from the previous release notes to keep router thresholds/timeouts constant.  
4. Rebuild & verify (`cargo test` + one of the verification runs above) before promoting the rollback.  
5. Notify stakeholders of the rollback with the new commit/tag and reason (include `step_result`/`llm_error` excerpts if helpful).  

If further documentation is needed (e.g., a deeper incident timeline), capture the log line IDs (the JSON `timestamp` + `event` fields) so you can correlate with external monitoring.

# Status

As of March 17, 2026, mbus is feature-complete enough to run and evaluate the local obstacle suite, but it still lacks checked-in real-model proof that the primary challenge gate is being met.

## Source Of Truth

- Primary success bar: `mbus challenge` on the default 12-task local obstacle suite in `harness/challenge/`.
- Required outcome: at least 10 passed tasks, structured screenshots/logs, token totals, cost totals, and a packaged report.
- Secondary regression bar: `mbus bench` on the 10-task local harness, passing at 8/10 or better.

## What Is Built

- Core browser agent loop with strict single-action validation, repair, routing, screenshots, and telemetry.
- `mbus bench` with both `scripted` and `openai` modes, token aggregation, and cost reporting.
- `mbus challenge` with observable-only task manifests, screenshot persistence, aggregate reports, and packaging.
- `mbus package` for portable challenge bundles.
- Supplemental adversarial fixtures in `harness/challenge_adversarial/` for prompt-injection-style and misleading-copy scenarios.

## What Still Counts As Incomplete

- Browser-backed validation is currently blocked in this environment by `cdp_launch_failed` during Chromium startup, so the proof path is not yet operational end to end.
- A lightweight browser startup preflight does not exist yet, so expensive `bench` and `challenge` runs can still fail late on avoidable runtime misconfiguration.
- No checked-in real-model challenge proof package exists in the repo.
- The primary gate is defined, but its current status must be established by a fresh local proof run after browser startup is stable.
- The supplemental adversarial slice exists, but it is not yet part of the primary release gate.

## Immediate Priorities

1. Stabilize the Chromium/CDP runtime used by local proof runs and browser-backed integration tests.
2. Add a lightweight browser startup preflight check before `mbus bench` and `mbus challenge`.
3. Generate the first real-model challenge proof package once the runtime prerequisites are in place.
4. Review the proof results and decide whether the 10/12 gate should stay as-is or be tightened.

## Canonical Proof Workflow

The canonical proof workflow remains:

```bash
MBUS_LLM_API_KEY=... \
MBUS_LLM_INPUT_COST_PER_MILLION=... \
MBUS_LLM_OUTPUT_COST_PER_MILLION=... \
./scripts/run_challenge_proof.sh
```

Before treating that workflow as the active next step, first confirm the browser runtime launches cleanly. In the current Codex environment, browser-backed validation is blocked by `cdp_launch_failed`, so runtime stabilization and a startup preflight are the active path to proof rather than optional follow-up work.

Expected outputs:
- `report.json` for the challenge run,
- an unpacked package directory,
- a zip archive with report, manifest, README, and artifacts.

Do not commit generated proof artifacts. Summarize the outcome in notes or status docs instead.

## Evidence Rules

- Product-level success must come from observable page state and user-like browser actions.
- Hidden app knowledge, bundle inspection, storage inspection, or direct-route shortcuts do not count as valid proof.
- Live-site evaluations are exploratory only unless they obey the same observable-only rules and produce reproducible evidence.

For the policy details, see `docs/live-eval-policy.md`.

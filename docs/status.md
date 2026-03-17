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

- No checked-in real-model challenge proof package exists in the repo.
- The primary gate is defined, but its current status must be established by a fresh local proof run.
- The supplemental adversarial slice exists, but it is not yet part of the primary release gate.

## Canonical Proof Workflow

Run:

```bash
MBUS_LLM_API_KEY=... \
MBUS_LLM_INPUT_COST_PER_MILLION=... \
MBUS_LLM_OUTPUT_COST_PER_MILLION=... \
./scripts/run_challenge_proof.sh
```

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

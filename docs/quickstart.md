# Install & Quickstart

A short, repeatable onboarding path for getting `mbus` built, then switching to the canonical challenge-proof flow.

## Quick path (minutes)
1. **Install prerequisites.** Rust stable (`rustup toolchain install stable`) plus a Chromium/Chrome binary discoverable by [chromiumoxide](https://github.com/mokus0/chromiumoxide).
2. **Build the CLI.** From the repo root run `cargo install --path . --bin mbus` or keep iterating with `cargo build`/`cargo run` (always pass `--bin mbus` because the workspace exposes multiple binaries).
3. **Run the stub task.** Start with the stub LLM to avoid needing API keys:
   ```bash
   MBUS_LLM_MODE=stub \
     cargo run --bin mbus -- run --task "open example.com" \
     --max-steps 1 --headless true
   ```
4. **Confirm success.** `mbus` prints JSON log lines ending with a `{"type":"summary","status":"done","terminal_state":"done"...}` record before exiting.

## Canonical challenge-proof path
After the stub smoke test, the primary product workflow is a real-model challenge run plus packaging:

```bash
MBUS_LLM_API_KEY=... \
MBUS_LLM_INPUT_COST_PER_MILLION=... \
MBUS_LLM_OUTPUT_COST_PER_MILLION=... \
./scripts/run_challenge_proof.sh
```

This workflow:
- runs `mbus challenge` against the default 12-task local obstacle suite,
- packages the resulting report and screenshots,
- prints the exact report and bundle paths for inspection.

If you want the raw commands instead of the helper script:

```bash
cargo run --bin mbus -- challenge --headless true --report-path target/challenge/report.json
cargo run --bin mbus -- package --report-path target/challenge/report.json
```

Use the helper script for reproducibility; use the raw commands only when iterating.

## Prerequisites
- **Rust toolchain.** Install via [rustup](https://rustup.rs/) and keep `rustup component add clippy rustfmt` handy for future development.
- **Chromium/Chrome.** `chromiumoxide` walks your `PATH` for a Chromium/Chrome binary. On macOS `brew install --cask google-chrome`, on Ubuntu/Debian `sudo apt install chromium-browser`, and on Fedora `sudo dnf install chromium`. If you already run a CDP-compatible browser elsewhere, set `MBUS_CDP_URL` or pass `--cdp-url` so `mbus run` attaches instead of launching its own.

## Build or install details
- Clone or update the repo:
  ```bash
  git clone https://github.com/<org>/mbus.git
  cd mbus
  ```
- One-off builds can be driven with `cargo build` or `cargo run --bin mbus -- <command>` while developing. For global availability, install the CLI via `cargo install --path . --bin mbus` and keep the repo for editing.
- Use `cargo install` again whenever dependencies or the source change significantly.

## First successful run explained
- The stub run above kicks the agent through one step and immediately emits a `done` action. No API keys are required.
- Output includes `type = config` lines, per-step JSON logs, and a final summary JSON record similar to the sample you saw when running the command (logs are printed to `stdout`).
- To inspect browser interactions, add `--headless false` or hook into `--cdp-url`/`MBUS_CDP_URL` so you can watch Chromium.

## Optional configuration (after quickstart)
- `mbus run` honors config in this precedence order: defaults → `--config` flag → `MBUS_CONFIG` env var → `./mbus.toml` → `~/.mbus.toml` → CLI flags. The config snippet in `README.md` shows the `agent`, `browser`, `router`, `validator`, `llm`, and `output` tables you can override.
- Environment overrides mirror every CLI flag (`MBUS_MAX_STEPS`, `MBUS_LLM_MODE`, `MBUS_LLM_API_KEY`, etc.). See `README.md` for the full list and map your preferred deployment parameters there.
- Switch out the stub LLM by setting `MBUS_LLM_MODE=openai` plus `MBUS_LLM_API_KEY=...` and tuning model names via `MBUS_LLM_MODEL_FAST`/`_MID`/`_STRONG`.
- For proof runs, also set `MBUS_LLM_INPUT_COST_PER_MILLION` and `MBUS_LLM_OUTPUT_COST_PER_MILLION` so packaged reports include cost totals.

## Validate the CLI surface
`cargo run --bin mbus -- run --help` lists every `mbus run` option with the same names you reference in scripts or docs; this command was executed successfully in the current workspace to confirm it still works.

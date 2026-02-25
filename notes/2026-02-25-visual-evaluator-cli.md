# 2026-02-25 Visual evaluator CLI spike

- Added `mbus visual` command that loads two `.ralph/runs/<run_id>` directories (baseline + candidate) and writes a deterministic JSON report at the requested path.
- Report schema v1 includes per-run summaries (run id + screenshot metadata), deterministic comparison scores per matching step, and placeholders for optional OCR entries.
- Utility stays outside the hot agent path; it purely relies on run artifacts already dumped under `.ralph/runs` and is insulated via its own `src/visual.rs` module plus clap subcommand wiring.
- Smoke test (`visual_cli_generates_report`) uses temporary directories to exercise argument parsing, report generation, and basic comparisons, satisfying the CLI parsing + report file verification request.

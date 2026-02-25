# 2026-02-25 REG-004 transition trace snippets

- Added a transition trace snippet schema plus artifact writer under `.ralph/runs/<run_id>/transition-trace.json` with minimal fields needed for escalation audits.
- CLI runs now persist trace snippets when router transitions exist and expose them as output artifacts with record counts and digests.
- Regression e2e tests for unchanged-state and repeat-validation loops write/parse trace snippets and assert expected transition tuples.
- Quality gate run: `cargo fmt --all -- --check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`.

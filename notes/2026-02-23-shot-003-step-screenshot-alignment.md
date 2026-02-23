# 2026-02-23 SHOT-003 step screenshot alignment fix

- Root cause: screenshot persistence used `step_screenshots` populated only after post-apply snapshots, so runs that ended on `done` (or validation failure before apply) dropped valid captured screenshot bytes for those steps.
- Reproduction: `cargo run --bin mbus -- run --task "noop" --llm-mode stub --screenshot-enabled true --screenshot-persist always --headless true` produced `output_artifacts: []` despite successful initial screenshot capture.
- Fix: track screenshot bytes for the current observation (`observation_screenshot`) and push that value for every recorded step; update it only after a new snapshot is captured.
- Added regression coverage in `src/agent/loop.rs` for:
  - done-first-step keeping screenshot bytes
  - screenshot sequence across apply -> next step
  - repeated validation failures reusing the same observation screenshot

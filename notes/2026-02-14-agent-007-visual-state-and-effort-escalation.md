# 2026-02-14 AGENT-007 visual-state and effort-escalation recommendation

- Context read:
  - `out.jsonl` run made early progress, then hit repeated no-progress interactions in unchanged state hash, and ended with an OpenAI timeout error.
  - Recent commit history shows concentrated churn on repeat-action guards, prompt context hardening, and element-id stability (`AGENT-005`, `AGENT-006`, `OBS-004`), indicating planner/perception quality remains the bottleneck.

- Recommendation 1 (state representation): use screenshots as a first-class signal, but not as a replacement for structured DOM state.
  - Keep existing actionable element map for deterministic execution (`click` by id).
  - Add a viewport screenshot per step to improve human-like scene understanding (layout, highlighted hints, visual cues not present in extracted text).
  - Keep `visible_text` and element metadata as a secondary channel and for validator invariants.

- Recommendation 2 (escalation): route on two axes, not only model tier.
  - Current routing escalates model tier on failure/no-progress streaks.
  - Add reasoning-effort escalation before/alongside model swaps (for example: `gpt-5.1 medium -> gpt-5.2 medium -> gpt-5.2 high`).
  - Trigger effort escalation specifically on unchanged-state streaks and repeated validation codes (for example `repeat_no_progress_action`), then reset after confirmed progress.

- Why this is likely to help:
  - Current no-progress loops happen even after prompt guardrails and stable ids, which suggests missing page understanding rather than id instability alone.
  - Screenshot-grounded reasoning matches the external test signal (ChatGPT solved the same step sequence from screenshot context).

- Missing tool idea:
  - Add a small local "visual diff + OCR extractor" utility for step artifacts (screenshot before/after, OCR text, changed regions) to benchmark whether image context materially improves decision quality.

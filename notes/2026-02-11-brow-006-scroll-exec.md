# 2026-02-11 BROW-006 bounded scroll execution

- Scroll execution now enforces `max_scroll` bounds before applying the delta and returns a structured `scroll_out_of_bounds` error if exceeded.
- Scroll actions emit the resulting `[scrollX, scrollY]` position in `StepResult.scroll` when available.
- The browser action applier reads its `max_scroll` from `CdpConfig`, and the agent wires the validator `max_scroll` into the browser config for consistency.

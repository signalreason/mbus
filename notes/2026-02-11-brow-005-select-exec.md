# 2026-02-11 BROW-005 select action execution

- Select now resolves options at runtime and accepts matches by value, label, or visible text.
- Invalid options return a structured `select_failed` error with `invalid_option`.
- The JS helper verifies the selected value after setting and reports `selection_mismatch` when it doesn't stick.

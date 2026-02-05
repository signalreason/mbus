# 2026-02-05 PROGRESS-001 no-progress heuristics

- Agent loop now computes progress using state hash, actionable element
  signatures, and low-actionability detection (<= 2 elements across
  consecutive snapshots).
- Unchanged actionable DOM (sorted element id signature) or low-actionability
  marks a step as no-progress to drive router escalation.
- Heuristic flags and actionable counts are emitted in step_result logs for
  debugging.

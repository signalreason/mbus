# 2026-02-11 M2-PROGRESS-002 low-actionability heuristic

- Added actionability scoring based on actionable element count and role weights.
- Low-actionability now triggers when the next observation has too few actionables
  or an actionability score below the threshold.
- Low-actionability streak escalates router tier independently of failure/no-progress
  counters, and the actionability score is logged in step results.

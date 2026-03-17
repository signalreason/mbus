# 2026-03-17 ALIGN-001 challenge-first source-of-truth

- The repo's primary success bar is now the 12-task local obstacle suite run through `mbus challenge`, not the 10-task bench harness.
- `mbus bench` remains important as regression coverage, but it is no longer the product-level outcome metric.
- Added `docs/status.md` as the current-state doc and `docs/live-eval-policy.md` as the evaluation-integrity policy.
- Added `scripts/run_challenge_proof.sh` so a real-model challenge run plus packaging can be reproduced without tribal knowledge.
- Added a supplemental adversarial tasks directory (`harness/challenge_adversarial/`) so prompt-injection-style and misleading-copy scenarios can be evaluated without moving the default 12-task gate.

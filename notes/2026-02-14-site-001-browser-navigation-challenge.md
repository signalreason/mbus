# 2026-02-14 - Site analysis: serene-frangipane challenge

Historical note. Superseded by `docs/live-eval-policy.md`. This note documents reverse-engineering observations and does not define valid product proof.

- Target site: `https://serene-frangipane-7fd25b.netlify.app`
- Bundle path observed: `/assets/index-BKEDqiCY.js`.
- The app stores challenge codes in `sessionStorage` under key `wo_session`.
- `class kn` initializes `codes` as 30 random 6-char values from `ABCDEFGHJKLMNPQRSTUVWXYZ23456789`.
- Flow checks:
  - `getCodeForStep(step)` returns code for `step` only if previous step is marked completed.
  - `markChallengeComplete(step, proof)` marks completion and returns `codes[step+1]`.
  - `validateCode(step, input)` checks `input === codes[step+1]`.
- Practical implication:
  - To advance from `/stepN`, submit the pre-generated `codes[N+1]`.
  - Challenge mechanics are mostly decoys once codes are known.
- Important bug in deployed bundle:
  - `codes` only contains entries 1..30, but `/step30` validates against `codes[31]`.
  - Therefore `validateCode(30, any)` is always false in current logic.
  - `/finish` route exists and is directly routable.
- Repo integration note:
  - `mbus bench` is hardwired to the local harness server; use `mbus run` (or a custom script) for external sites.

# 2026-02-14 - Live site revalidation: serene-frangipane challenge

- Revalidated live target at `https://serene-frangipane-7fd25b.netlify.app` on 2026-02-14.
- Landing page still loads bundle `/assets/index-BKEDqiCY.js`.
- Bundle still contains `wo_session` session storage flow and `codes` generation for 30 entries.
- Validation path still uses `validateCode(step, input)` with lookup `codes[step+1]`.
- Consequence remains unchanged:
  - `/step30` validates against `codes[31]`, which is absent, so step-30 code submission is impossible via normal validation.
  - `/finish` route exists and remains directly routable.
- `mbus` integration reminder:
  - `mbus bench` is for local harness fixtures only.
  - External challenge site must be run with `mbus run` and `--initial-url`.

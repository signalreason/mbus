# 2026-02-14 Site-003 netlify entrypoint and empty-content failure

- Direct load of `https://serene-frangipane-7fd25b.netlify.app/step1` returns Netlify `404 Page not found`.
- The SPA defines `/step1` client routes, but direct navigation requires host rewrite rules that are not configured.
- For `mbus run`, set `initial_url` to `https://serene-frangipane-7fd25b.netlify.app/` and let the app route internally.
- Observed failure `invalid_json: empty response` is consistent with model content returning empty text under tight completion budget.
- Increasing `llm.max_tokens` from `256` to `1024` reduces blank-output risk for GPT-5 chat models with large observations.

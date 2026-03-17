# Live Evaluation Policy

Live-site evaluation is exploratory. The canonical release proof for mbus is the local obstacle suite run through `mbus challenge`.

## What Counts

- Using the browser the way a human operator would: navigate, click, type, select, scroll, wait, and back.
- Relying on visible text, reachable controls, page URL, and persisted screenshots as evidence.
- Packaging the resulting report so another operator can inspect the same outcome.

## What Does Not Count

- Reading JavaScript bundles, source maps, or app internals to discover answers.
- Reading `sessionStorage`, `localStorage`, cookies, or network payloads for hidden state that is not browser-visible to the operator.
- Navigating directly to hidden routes or completion pages that a human would not discover from the page flow.
- Injecting deterministic site-specific knowledge that bypasses the observable interaction problem.

If a live site can only be solved with those techniques, it is not valid product proof for mbus.

## Relationship To `challenge.toml`

`challenge.toml` is an exploratory live-eval example only. It is useful for local experimentation against a specific site, but it is not the authoritative success criterion for the repo.

## Recommended Practice

- Use the local 12-task obstacle suite for release claims.
- Use supplemental adversarial fixtures to pressure-test prompt handling.
- Use live sites only to gather exploratory evidence and future fixture ideas, then convert those lessons into local observable-only tasks where possible.

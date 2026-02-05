# VERIFY-001 action validator

- Added `Validator` with configurable limits: `allow_insecure`, `max_text_len`, `max_wait_ms`, `max_scroll`.
- Rules enforced:
  - `id` required and must exist for click/type/select and extract with id.
  - `Type.text` length <= 2000 (default).
  - `Wait.ms` <= 30000 (default).
  - `Scroll.dx/dy` bounded to +/- 2000 (default).
  - `Navigate.url` must be http/https unless `allow_insecure`.
- Error codes used: `missing_id`, `unknown_id`, `text_too_long`, `wait_too_long`, `scroll_out_of_bounds`, `insecure_url`, `missing_url`.
- Validator returns ordered errors for determinism.

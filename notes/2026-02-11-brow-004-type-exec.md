# 2026-02-11 BROW-004 type action execution

- Type now attempts form submission when `submit` is true by calling `requestSubmit` on the owning form, falling back to Enter when no form is present.
- Submit errors other than missing form are surfaced as `submit_failed`.
- Added e2e coverage for submit via a harness form.

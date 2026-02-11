# M0-SCHEMA-002 golden action payloads

- Added canonical JSON fixtures under `tests/fixtures/actions/` for all action types.
- Added one golden round-trip test per action type in `tests/action_goldens.rs`.
- Round-trip asserts strict deserialize + serialize stability on the golden payloads.

# 2026-02-24 SHOT-004 observation screenshot metadata

- Added optional `Observation.screenshot` metadata (mime type, sha256, byte size, optional artifact ref).
- Populated screenshot metadata from captured bytes in the agent loop without changing element-based validation.
- Added serialization coverage for observations with and without screenshot metadata.

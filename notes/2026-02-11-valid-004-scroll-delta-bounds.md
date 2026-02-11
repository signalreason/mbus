# 2026-02-11 VALID-004 scroll delta bounds validation

- Added shared `exceeds_symmetric_limit_i64` guard for scroll delta bounds.
- Validator and scroll executor now use the same symmetric bound check.
- Validator tests cover configured scroll bounds and out-of-range rejection.

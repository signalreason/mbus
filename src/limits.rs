pub fn exceeds_symmetric_limit_i64(value: i64, limit: i64) -> bool {
    let bound = limit.saturating_abs();
    value.saturating_abs() > bound
}

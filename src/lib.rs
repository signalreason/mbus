pub mod config;
pub mod agent;
pub mod browser;
pub mod llm;
pub mod verify;
pub mod types;
pub mod telemetry;

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_builds() {
        assert_eq!(2 + 2, 4);
    }
}

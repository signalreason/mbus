pub mod agent;
pub mod bench;
pub mod browser;
pub mod config;
pub mod llm;
pub mod output;
pub mod telemetry;
pub mod types;
pub mod verify;

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_builds() {
        assert_eq!(2 + 2, 4);
    }
}

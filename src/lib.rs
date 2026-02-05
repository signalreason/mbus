pub mod agent;
pub mod browser;
pub mod llm;
pub mod verify;
pub mod types;

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_builds() {
        assert_eq!(2 + 2, 4);
    }
}

use crate::agent::memory::MemoryConfig;

#[derive(Clone, Debug)]
pub struct AgentPolicy {
    pub max_steps: usize,
    pub max_no_progress_steps: usize,
    pub memory: MemoryConfig,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        Self {
            max_steps: 40,
            max_no_progress_steps: 8,
            memory: MemoryConfig::default(),
        }
    }
}

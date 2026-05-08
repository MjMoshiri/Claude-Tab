use crate::workflow::ir::Workflow;
use std::collections::HashMap;
use std::sync::RwLock;

pub struct WorkflowRegistry {
    inner: RwLock<HashMap<String, Workflow>>,
}

impl WorkflowRegistry {
    pub fn new() -> Self {
        Self { inner: RwLock::new(HashMap::new()) }
    }

    pub fn upsert(&self, wf: Workflow) {
        self.inner.write().unwrap().insert(wf.id.clone(), wf);
    }

    pub fn get(&self, id: &str) -> Option<Workflow> {
        self.inner.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Workflow> {
        self.inner.read().unwrap().values().cloned().collect()
    }
}

impl Default for WorkflowRegistry {
    fn default() -> Self {
        Self::new()
    }
}

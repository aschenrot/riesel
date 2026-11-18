use std::collections::HashMap;
use std::process::Child;

#[expect(dead_code, reason = "Process Manager implementation is pending")]
pub struct ManagedProcess {
    child: Child,
}

#[expect(dead_code, reason = "Process Manager implementation is pending")]
pub struct ProcessKey {
    instance_id: u64,
    name: Option<String>,
}

#[expect(dead_code, reason = "Process Manager implementation is pending")]
pub struct ProcessRegistry {
    processes: HashMap<ProcessKey, ManagedProcess>,
}

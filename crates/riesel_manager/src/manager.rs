use std::collections::HashMap;
use std::process::Child;

pub struct ManagedProcess {
    child: Child,
}

pub struct ProcessKey {
    instance_id: u64,
    name: Option<String>,
}

pub struct ProcessRegistry {
    processes: HashMap<ProcessKey, ManagedProcess>,
}

//! Capability macro argument parsing, code generation and signature checks.
mod args;
mod expand;
mod signature;
#[cfg(test)]
mod tests;

pub(crate) use args::CapabilityArgs;
pub(crate) use expand::expand_capability;

//! Thin executable host using the same component definition as local use.

use crate::memory_counter_provider;
use std::io;

pub fn run_counter_server(address: &str) -> io::Result<()> {
    memory_counter_provider().map_err(io::Error::other)?.serve(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_loopback_counter_server() {
        let err = run_counter_server("0.0.0.0:0").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}

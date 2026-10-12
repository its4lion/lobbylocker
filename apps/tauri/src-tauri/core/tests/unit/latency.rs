//! Unit tests for the latency module.
use super::*;
#[test]
fn parses_linux_and_windows() {
    assert_eq!(parse_ping("64 bytes time=12.6 ms ttl=54"), Some(12.6));
    assert_eq!(parse_ping("time=42 ms"), Some(42.0));
    assert_eq!(parse_ping("time<1 ms"), Some(1.0));
    assert_eq!(parse_ping("Request timed out."), None);
}

//! Unit tests for the latency module.
use super::*;
#[test]
fn parses_icmp_output() {
    assert_eq!(parse_ping("64 bytes time=12.6 ms ttl=54"), Some(12.6));
    assert_eq!(parse_ping("time=42 ms"), Some(42.0));
    assert_eq!(parse_ping("time<1 ms"), Some(1.0));
    assert_eq!(parse_ping("Request timed out."), None);
}

#[test]
fn rejects_hostnames_and_commands_before_starting_a_probe() {
    for target in ["example.com", "127.0.0.1; calc.exe", "203.0.113.7/24", ""] {
        assert_eq!(
            ping(target).unwrap_err(),
            "Ping target must be a literal IP address."
        );
    }
}

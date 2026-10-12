//! Unit tests for the connections module.
use super::*;
#[test]
fn parses_ipv4_ipv6_and_ignores_wildcards() {
    let result = parse_ss("tcp ESTAB 0 0 192.168.1.1:45000 203.0.113.7:27015 users:((\"game\",pid=12,fd=5))\nudp ESTAB 0 0 [::]:5000 [2001:db8::5]:27016\nudp UNCONN 0 0 0.0.0.0:27015 0.0.0.0:*");
    assert_eq!(result.len(), 2);
    assert_eq!(result[1].remote_ip, "2001:db8::5");
}

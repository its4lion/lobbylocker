//! Unit tests for the connections module.
use super::*;
#[test]
fn parses_ipv4_ipv6_and_ignores_wildcards() {
    let result = parse_ss("tcp ESTAB 0 0 192.168.1.1:45000 203.0.113.7:27015 users:((\"game\",pid=12,fd=5))\nudp ESTAB 0 0 [::]:5000 [2001:db8::5]:27016\nudp UNCONN 0 0 0.0.0.0:27015 0.0.0.0:*");
    assert_eq!(result.len(), 2);
    assert_eq!(result[1].remote_ip, "2001:db8::5");
}

#[test]
fn keeps_distinct_processes_and_states_for_the_same_remote_endpoint() {
    let output = concat!(
        "tcp ESTAB 0 0 192.0.2.1:1 203.0.113.7:443 users:((\"first\",pid=1,fd=2))\n",
        "tcp ESTAB 0 0 192.0.2.1:2 203.0.113.7:443 users:((\"second\",pid=2,fd=3))\n",
        "tcp CLOSE-WAIT 0 0 192.0.2.1:3 203.0.113.7:443 users:((\"second\",pid=2,fd=3))\n",
        "tcp ESTAB 0 0 192.0.2.1:1 203.0.113.7:443 users:((\"first\",pid=1,fd=2))\n",
    );
    let results = parse_ss(output);
    assert_eq!(results.len(), 3);
    assert!(results.iter().any(|c| c.process.contains("first")));
    assert!(results
        .iter()
        .any(|c| c.process.contains("second") && c.state == "CLOSE-WAIT"));
}

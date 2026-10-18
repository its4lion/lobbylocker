use crate::{model::Game, Result};
use serde::Serialize;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::{
    collections::VecDeque,
    net::IpAddr,
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PingSample {
    pub region_key: String,
    pub target: String,
    pub latency_ms: Option<f64>,
    pub error: Option<String>,
    pub measured_at: u64,
}

pub fn parse_ping(output: &str) -> Option<f64> {
    let value = output
        .split("time=")
        .nth(1)
        .or_else(|| output.split("time<").nth(1))?;
    value
        .split_whitespace()
        .next()?
        .trim_end_matches("ms")
        .parse()
        .ok()
}

pub fn ping(target: &str) -> Result<f64> {
    let address: IpAddr = target
        .parse()
        .map_err(|_| "Ping target must be a literal IP address.")?;
    #[cfg(not(target_os = "windows"))]
    let _ = address;
    #[cfg(target_os = "linux")]
    let output = Command::new("ping")
        .env("LC_ALL", "C")
        .args(["-n", "-c", "1", "-W", "2", "--", target])
        .output()
        .map_err(|e| format!("Could not run ping: {e}"))?;
    #[cfg(target_os = "windows")]
    return windows_ping::ping(address);
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    return Err("Ping is currently supported on Windows and Linux.".into());
    #[cfg(target_os = "linux")]
    {
        if !output.status.success() {
            return Err("No ICMP reply (filtered, blocked, or unreachable).".into());
        }
        parse_ping(&String::from_utf8_lossy(&output.stdout))
            .ok_or("Could not read ping response.".into())
    }
}

#[cfg(target_os = "windows")]
mod windows_ping {
    use super::*;
    use windows::Win32::{
        Foundation::HANDLE,
        NetworkManagement::IpHelper::{
            Icmp6CreateFile, Icmp6SendEcho2, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
            ICMPV6_ECHO_REPLY_LH, ICMP_ECHO_REPLY,
        },
        Networking::WinSock::{AF_INET6, IN6_ADDR, IN6_ADDR_0, SOCKADDR_IN6},
    };

    struct IcmpHandle(HANDLE);

    impl Drop for IcmpHandle {
        fn drop(&mut self) {
            // ICMP handles require their own close API.
            let _ = unsafe { IcmpCloseHandle(self.0) };
        }
    }

    pub(super) fn ping(address: IpAddr) -> Result<f64> {
        let request = [0u8; 32];
        // Aligned storage for either reply header, payload, ICMP error and IO_STATUS_BLOCK.
        let mut reply = [0u64; 32];
        let reply_size = std::mem::size_of_val(&reply) as u32;
        // The reply buffer stays unset until at least one reply arrives; never
        // interpret it after an empty result.
        let responded = match address {
            IpAddr::V4(address) => {
                let handle = IcmpHandle(
                    unsafe { IcmpCreateFile() }
                        .map_err(|e| format!("Could not initialize ICMP: {e}"))?,
                );
                let count = unsafe {
                    IcmpSendEcho(
                        handle.0,
                        u32::from_ne_bytes(address.octets()),
                        request.as_ptr().cast(),
                        request.len() as u16,
                        None,
                        reply.as_mut_ptr().cast(),
                        reply_size,
                        2000,
                    )
                };
                (count != 0).then(|| {
                    let response =
                        unsafe { reply.as_ptr().cast::<ICMP_ECHO_REPLY>().read_unaligned() };
                    (response.Status, response.RoundTripTime)
                })
            }
            IpAddr::V6(address) => {
                let handle = IcmpHandle(
                    unsafe { Icmp6CreateFile() }
                        .map_err(|e| format!("Could not initialize IPv6 ICMP: {e}"))?,
                );
                let source = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    ..Default::default()
                };
                let destination = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    sin6_addr: IN6_ADDR {
                        u: IN6_ADDR_0 {
                            Byte: address.octets(),
                        },
                    },
                    ..Default::default()
                };
                let count = unsafe {
                    Icmp6SendEcho2(
                        handle.0,
                        None,
                        None,
                        None,
                        &source,
                        &destination,
                        request.as_ptr().cast(),
                        request.len() as u16,
                        None,
                        reply.as_mut_ptr().cast(),
                        reply_size,
                        2000,
                    )
                };
                (count != 0).then(|| {
                    let response = unsafe {
                        reply
                            .as_ptr()
                            .cast::<ICMPV6_ECHO_REPLY_LH>()
                            .read_unaligned()
                    };
                    (response.Status, response.RoundTripTime)
                })
            }
        };
        let Some((status, round_trip)) = responded.filter(|(status, _)| *status == 0) else {
            return Err("No ICMP reply (filtered, blocked, or unreachable).".into());
        };
        Ok(f64::from(round_trip))
    }
}

pub fn measure(games: &[Game]) -> Vec<PingSample> {
    let targets: VecDeque<_> = games
        .iter()
        .flat_map(|g| {
            g.regions.iter().filter_map(|r| {
                r.probe_target
                    .as_ref()
                    .map(|t| (format!("{}:{}", g.id, r.id), t.clone()))
            })
        })
        .collect();
    let queue = Arc::new(Mutex::new(targets));
    let results = Arc::new(Mutex::new(Vec::new()));
    thread::scope(|scope| {
        for _ in 0..8 {
            let queue = queue.clone();
            let results = results.clone();
            scope.spawn(move || loop {
                let Some((region_key, target)) = queue.lock().unwrap().pop_front() else {
                    break;
                };
                let result = ping(&target);
                let measured_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let sample = PingSample {
                    region_key,
                    target,
                    latency_ms: result.as_ref().ok().copied(),
                    error: result.err(),
                    measured_at,
                };
                results.lock().unwrap().push(sample);
            });
        }
    });
    Arc::try_unwrap(results).unwrap().into_inner().unwrap()
}

#[cfg(test)]
#[path = "../tests/unit/latency.rs"]
mod tests;

use crate::{model::Game, Result};
use serde::Serialize;
use std::{
    collections::VecDeque,
    net::IpAddr,
    process::Command,
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
    let _: IpAddr = target
        .parse()
        .map_err(|_| "Ping target must be a literal IP address.")?;
    #[cfg(target_os = "linux")]
    let output = Command::new("ping")
        .env("LC_ALL", "C")
        .args(["-n", "-c", "1", "-W", "2", "--", target])
        .output()
        .map_err(|e| format!("Could not run ping: {e}"))?;
    #[cfg(target_os = "windows")]
    let output = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &format!("$p=New-Object System.Net.NetworkInformation.Ping; try {{ $r=$p.Send('{target}',2000); if ($r.Status -eq 'Success') {{ Write-Output ('time=' + $r.RoundtripTime + ' ms') }} else {{ exit 1 }} }} finally {{ $p.Dispose() }}")]).output().map_err(|e| format!("Could not run ping: {e}"))?;
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    return Err("Ping is currently supported on Windows and Linux.".into());
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        if !output.status.success() {
            return Err("No ICMP reply (filtered, blocked, or unreachable).".into());
        }
        parse_ping(&String::from_utf8_lossy(&output.stdout))
            .ok_or("Could not read ping response.".into())
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

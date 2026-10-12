use crate::Result;
use serde::Serialize;
use std::{collections::HashSet, net::IpAddr, process::Command};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub protocol: String,
    pub remote_ip: String,
    pub remote_port: u16,
    pub process: String,
    pub state: String,
}

pub fn parse_address(raw: &str) -> Option<(IpAddr, u16)> {
    let (ip, port) = raw.rsplit_once(':')?;
    let ip: IpAddr = ip.trim_matches(['[', ']']).parse().ok()?;
    let port: u16 = port.parse().ok()?;
    if port == 0 || ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return None;
    }
    Some((ip, port))
}

pub fn parse_ss(output: &str) -> Vec<Connection> {
    let mut seen = HashSet::new();
    output
        .lines()
        .filter_map(|line| {
            let cols: Vec<_> = line.split_whitespace().collect();
            if cols.len() < 6 {
                return None;
            }
            let (ip, port) = parse_address(cols[5])?;
            if !seen.insert((cols[0].to_string(), ip, port)) {
                return None;
            }
            Some(Connection {
                protocol: cols[0].into(),
                state: cols[1].into(),
                remote_ip: ip.to_string(),
                remote_port: port,
                process: cols.get(6..).unwrap_or_default().join(" "),
            })
        })
        .collect()
}

pub fn scan() -> Result<Vec<Connection>> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("ss")
            .args(["-H", "-n", "-t", "-u", "-p"])
            .output()
            .map_err(|e| format!("Could not run ss (install iproute2): {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into());
        }
        Ok(parse_ss(&String::from_utf8_lossy(&output.stdout)))
    }
    #[cfg(target_os = "windows")]
    {
        // Windows' UDP endpoint table has no remote peer information. Do not fabricate it.
        let script = "Get-NetTCPConnection -State Established -ErrorAction Stop | ForEach-Object { $p=Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue; Write-Output ('tcp|' + $_.RemoteAddress + '|' + $_.RemotePort + '|' + $p.ProcessName + '|ESTABLISHED') }";
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into());
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let cols: Vec<_> = line.split('|').collect();
                if cols.len() != 5 {
                    return None;
                }
                let (ip, port) = parse_address(&format!("{}:{}", cols[1], cols[2]))?;
                Some(Connection {
                    protocol: cols[0].into(),
                    remote_ip: ip.to_string(),
                    remote_port: port,
                    process: cols[3].into(),
                    state: cols[4].into(),
                })
            })
            .collect())
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    Err("Socket discovery is supported on Windows and Linux only.".into())
}

#[cfg(test)]
#[path = "../tests/unit/connections.rs"]
mod tests;

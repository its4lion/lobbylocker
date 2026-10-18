use crate::Result;
use serde::Serialize;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::{collections::HashSet, net::IpAddr};

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
    remote_address(ip, port)
}

fn remote_address(ip: IpAddr, port: u16) -> Option<(IpAddr, u16)> {
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
        windows_connections::scan()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    Err("Socket discovery is supported on Windows and Linux only.".into())
}

#[cfg(test)]
#[path = "../tests/unit/connections.rs"]
mod tests;

#[cfg(target_os = "windows")]
mod windows_connections {
    use super::*;
    use std::{collections::HashMap, mem, net::Ipv4Addr, path::Path};
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HANDLE},
            NetworkManagement::IpHelper::{
                GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID,
                MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, MIB_TCP_STATE_ESTAB,
                TCP_TABLE_OWNER_PID_CONNECTIONS,
            },
            Networking::WinSock::{ADDRESS_FAMILY, AF_INET, AF_INET6},
            System::Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };

    struct ProcessHandle(HANDLE);

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn process_name(pid: u32) -> String {
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return String::new();
        };
        let handle = ProcessHandle(handle);
        let mut buffer = vec![0u16; 32_768];
        let mut length = buffer.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(
                handle.0,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
        }
        .is_err()
        {
            // Processes may exit between taking the snapshot and looking up their names.
            return String::new();
        }
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        Path::new(&path)
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn tcp_table(family: ADDRESS_FAMILY) -> Result<(Vec<u32>, usize)> {
        let mut size = 0;
        let mut code = unsafe {
            GetExtendedTcpTable(
                None,
                &mut size,
                false,
                u32::from(family.0),
                TCP_TABLE_OWNER_PID_CONNECTIONS,
                0,
            )
        };
        // The table can grow between calls. Retry only when the API asks for a larger buffer.
        for _ in 0..4 {
            if code != ERROR_INSUFFICIENT_BUFFER.0 && code != ERROR_SUCCESS.0 {
                break;
            }
            if !(4..=16_777_216).contains(&size) {
                return Err("Windows returned an invalid TCP table size.".into());
            }
            let mut table = vec![0u32; (size as usize).div_ceil(mem::size_of::<u32>())];
            code = unsafe {
                GetExtendedTcpTable(
                    Some(table.as_mut_ptr().cast()),
                    &mut size,
                    false,
                    u32::from(family.0),
                    TCP_TABLE_OWNER_PID_CONNECTIONS,
                    0,
                )
            };
            if code == ERROR_SUCCESS.0 {
                return Ok((table, size as usize));
            }
        }
        Err(format!(
            "Could not read Windows TCP connections: {}",
            std::io::Error::from_raw_os_error(code as i32)
        ))
    }

    /// Read the bounded native table, including the layout's header padding.
    ///
    /// # Safety
    /// `T` must be the native row type for the address family used to obtain `table`.
    unsafe fn rows<T: Copy>(table: &[u32], length: usize, offset: usize) -> Result<Vec<T>> {
        let length = length.min(mem::size_of_val(table));
        let count = table.first().copied().unwrap_or(0) as usize;
        if offset > length || count > (length - offset) / mem::size_of::<T>() {
            return Err("Windows returned an invalid TCP connection table.".into());
        }
        let start = unsafe { table.as_ptr().cast::<u8>().add(offset) };
        Ok((0..count)
            .map(|index| unsafe {
                start
                    .add(index * mem::size_of::<T>())
                    .cast::<T>()
                    .read_unaligned()
            })
            .collect())
    }

    fn push_connection(
        connections: &mut Vec<Connection>,
        processes: &mut HashMap<u32, String>,
        ip: IpAddr,
        port: u32,
        state: u32,
        pid: u32,
    ) {
        if state != MIB_TCP_STATE_ESTAB.0 as u32 {
            return;
        }
        // The remote port occupies the low 16 bits in network byte order.
        let Some((ip, port)) = remote_address(ip, u16::from_be(port as u16)) else {
            return;
        };
        connections.push(Connection {
            protocol: "tcp".into(),
            remote_ip: ip.to_string(),
            remote_port: port,
            process: processes
                .entry(pid)
                .or_insert_with(|| process_name(pid))
                .clone(),
            state: "ESTABLISHED".into(),
        });
    }

    pub(super) fn scan() -> Result<Vec<Connection>> {
        let mut connections = Vec::new();
        let mut processes = HashMap::new();
        let (table, length) = tcp_table(AF_INET)?;
        let ipv4 = unsafe {
            rows::<MIB_TCPROW_OWNER_PID>(
                &table,
                length,
                mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table),
            )?
        };
        for row in ipv4 {
            push_connection(
                &mut connections,
                &mut processes,
                IpAddr::V4(Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes())),
                row.dwRemotePort,
                row.dwState,
                row.dwOwningPid,
            );
        }
        let (table, length) = tcp_table(AF_INET6)?;
        let ipv6 = unsafe {
            rows::<MIB_TCP6ROW_OWNER_PID>(
                &table,
                length,
                mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table),
            )?
        };
        for row in ipv6 {
            push_connection(
                &mut connections,
                &mut processes,
                IpAddr::V6(row.ucRemoteAddr.into()),
                row.dwRemotePort,
                row.dwState,
                row.dwOwningPid,
            );
        }
        // Windows' UDP endpoint table has no remote peers; do not invent connections.
        Ok(connections)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn decodes_network_order_ports_and_filters_non_connections() {
            let mut connections = Vec::new();
            let mut processes = HashMap::from([(123, "game".into())]);
            for (ip, port, state) in [
                ("203.0.113.7", 27015, 5),
                ("2001:db8::5", 443, 5),
                ("127.0.0.1", 80, 5),
                ("0.0.0.0", 80, 5),
                ("224.0.0.1", 80, 5),
                ("203.0.113.8", 0, 5),
                ("203.0.113.9", 80, 2),
            ] {
                push_connection(
                    &mut connections,
                    &mut processes,
                    ip.parse().unwrap(),
                    u32::from((port as u16).to_be()),
                    state,
                    123,
                );
            }
            assert_eq!(connections.len(), 2);
            assert_eq!(connections[0].remote_ip, "203.0.113.7");
            assert_eq!(connections[0].remote_port, 27015);
            assert_eq!(connections[1].remote_ip, "2001:db8::5");
            assert_eq!(connections[1].remote_port, 443);
            assert_eq!(connections[0].process, "game");
        }

        #[test]
        fn rejects_truncated_native_tables() {
            let table = [2u32, 0, 0, 0, 0, 0, 0];
            let result =
                unsafe { rows::<MIB_TCPROW_OWNER_PID>(&table, mem::size_of_val(&table), 4) };
            assert!(result.is_err());
        }

        #[test]
        fn reads_native_ipv4_rows_after_the_header() {
            let address = u32::from_ne_bytes([203, 0, 113, 7]);
            let port = u32::from(27015u16.to_be());
            let table = [1u32, 5, 0, 0, address, port, 123];
            let result = unsafe {
                rows::<MIB_TCPROW_OWNER_PID>(
                    &table,
                    mem::size_of_val(&table),
                    mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table),
                )
            }
            .unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].dwRemoteAddr, address);
            assert_eq!(result[0].dwRemotePort, port);
            assert_eq!(result[0].dwOwningPid, 123);
        }
    }
}

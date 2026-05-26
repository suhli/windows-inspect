use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use sysinfo::{Pid, ProcessesToUpdate, System};
use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
    TCP_TABLE_OWNER_PID_ALL,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TcpConnection {
    pub remote_ip: String,
    pub remote_port: u16,
    pub owning_pid: u32,
}

#[derive(Clone, Debug)]
pub struct IpPortGroup {
    pub remote_ip: String,
    pub remote_port: u16,
    pub count: usize,
}

#[derive(Clone, Debug)]
pub struct IpProcessGroup {
    pub remote_ip: String,
    pub process_name: String,
    pub count: usize,
}

#[derive(Clone, Debug)]
pub struct TcpSnapshot {
    pub total_count: usize,
    pub by_ip_port: Vec<IpPortGroup>,
    pub by_ip_process: Vec<IpProcessGroup>,
}

pub fn capture_tcp_snapshot() -> Result<TcpSnapshot, String> {
    let mut connections = Vec::new();
    connections.extend(read_tcp_v4()?);
    connections.extend(read_tcp_v6()?);

    let total_count = connections.len();
    let by_ip_port = group_by_ip_port(&connections);
    let by_ip_process = group_by_ip_process(&connections)?;

    Ok(TcpSnapshot {
        total_count,
        by_ip_port,
        by_ip_process,
    })
}

fn read_tcp_v4() -> Result<Vec<TcpConnection>, String> {
    let table = fetch_tcp_table::<MIB_TCPTABLE_OWNER_PID>(AF_INET.0.into())?;
    let rows = unsafe {
        std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
    };

    Ok(rows
        .iter()
        .filter_map(parse_tcp_v4_row)
        .collect())
}

fn read_tcp_v6() -> Result<Vec<TcpConnection>, String> {
    use windows::Win32::NetworkManagement::IpHelper::MIB_TCP6TABLE_OWNER_PID;

    let table = fetch_tcp_table::<MIB_TCP6TABLE_OWNER_PID>(AF_INET6.0.into())?;
    let rows = unsafe {
        std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
    };

    Ok(rows
        .iter()
        .filter_map(parse_tcp_v6_row)
        .collect())
}

fn fetch_tcp_table<T>(address_family: u32) -> Result<Box<T>, String> {
    let mut size = 0u32;
    unsafe {
        let _ = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            address_family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
    }

    if size == 0 {
        return Err("GetExtendedTcpTable 返回大小为 0".into());
    }

    let mut buffer = vec![0u8; size as usize];
    loop {
        let ret = unsafe {
            GetExtendedTcpTable(
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
                false,
                address_family,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        };

        if ret == 0 {
            break;
        }
        if ret == ERROR_INSUFFICIENT_BUFFER.0 {
            buffer.resize(size as usize, 0);
            continue;
        }
        return Err(format!("GetExtendedTcpTable 失败，错误码: {ret}"));
    }

    let table = buffer.into_boxed_slice();
    let table_ptr = Box::into_raw(table) as *mut T;
    Ok(unsafe { Box::from_raw(table_ptr) })
}

fn parse_tcp_v4_row(row: &MIB_TCPROW_OWNER_PID) -> Option<TcpConnection> {
    let remote_ip = ipv4_to_string(row.dwRemoteAddr);
    if is_unspecified_v4(&remote_ip) {
        return None;
    }

    Some(TcpConnection {
        remote_ip,
        remote_port: port_from_dw(row.dwRemotePort),
        owning_pid: row.dwOwningPid,
    })
}

fn parse_tcp_v6_row(
    row: &windows::Win32::NetworkManagement::IpHelper::MIB_TCP6ROW_OWNER_PID,
) -> Option<TcpConnection> {
    let remote_ip = ipv6_to_string(&row.ucRemoteAddr);
    if is_unspecified_v6(&remote_ip) {
        return None;
    }

    Some(TcpConnection {
        remote_ip,
        remote_port: port_from_dw(row.dwRemotePort),
        owning_pid: row.dwOwningPid,
    })
}

fn group_by_ip_port(connections: &[TcpConnection]) -> Vec<IpPortGroup> {
    let mut counts: HashMap<(String, u16), usize> = HashMap::new();
    for conn in connections {
        *counts
            .entry((conn.remote_ip.clone(), conn.remote_port))
            .or_default() += 1;
    }

    let mut groups: Vec<IpPortGroup> = counts
        .into_iter()
        .map(|((remote_ip, remote_port), count)| IpPortGroup {
            remote_ip,
            remote_port,
            count,
        })
        .collect();

    groups.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.remote_ip.cmp(&b.remote_ip))
            .then_with(|| a.remote_port.cmp(&b.remote_port))
    });
    groups
}

fn group_by_ip_process(connections: &[TcpConnection]) -> Result<Vec<IpProcessGroup>, String> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let mut counts: HashMap<(String, String), usize> = HashMap::new();
    for conn in connections {
        let process_name = resolve_process_name(&system, conn.owning_pid);
        *counts
            .entry((conn.remote_ip.clone(), process_name))
            .or_default() += 1;
    }

    let mut groups: Vec<IpProcessGroup> = counts
        .into_iter()
        .map(|((remote_ip, process_name), count)| IpProcessGroup {
            remote_ip,
            process_name,
            count,
        })
        .collect();

    groups.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.remote_ip.cmp(&b.remote_ip))
            .then_with(|| a.process_name.cmp(&b.process_name))
    });
    Ok(groups)
}

fn resolve_process_name(system: &System, pid: u32) -> String {
    if pid == 0 {
        return "System".to_string();
    }

    system
        .process(Pid::from_u32(pid))
        .map(|p| p.name().to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("PID {pid}"))
}

fn ipv4_to_string(addr: u32) -> String {
    Ipv4Addr::from(addr.to_be()).to_string()
}

fn ipv6_to_string(addr: &[u8; 16]) -> String {
    Ipv6Addr::from(*addr).to_string()
}

fn port_from_dw(port: u32) -> u16 {
    u16::from_be((port & 0xFFFF) as u16)
}

fn is_unspecified_v4(ip: &str) -> bool {
    ip == "0.0.0.0"
}

fn is_unspecified_v6(ip: &str) -> bool {
    ip == "::"
}

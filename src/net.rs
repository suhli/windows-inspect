use std::collections::HashMap;
use std::mem::size_of;
use std::net::{Ipv4Addr, Ipv6Addr};

use sysinfo::{Pid, ProcessesToUpdate, System};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, TRUE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetIfTable, GetPerTcp6ConnectionEStats, GetPerTcpConnectionEStats,
    MIB_IFTABLE, MIB_TCP6ROW, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID,
    MIB_TCPROW_LH, MIB_TCPROW_LH_0, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
    SetPerTcp6ConnectionEStats, SetPerTcpConnectionEStats, TCP_ESTATS_DATA_ROD_v0,
    TCP_ESTATS_DATA_RW_v0, TCP_TABLE_OWNER_PID_ALL, TcpConnectionEstatsData,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, IN6_ADDR};

use crate::speed::{ConnKey, ConnSpeed, ConnectionSample, SpeedTracker};

pub use crate::speed::{build_line_path, format_speed, SpeedTracker};

const MIB_TCP_STATE_ESTAB: u32 = 5;
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;

#[derive(Clone, Debug)]
pub struct TcpConnection {
    pub key: ConnKey,
    pub remote_ip: String,
    pub remote_port: u16,
    pub owning_pid: u32,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Clone, Debug)]
pub struct IpPortGroup {
    pub remote_ip: String,
    pub remote_port: u16,
    pub count: usize,
    pub down_bps: u64,
    pub up_bps: u64,
}

#[derive(Clone, Debug)]
pub struct IpProcessGroup {
    pub remote_ip: String,
    pub process_name: String,
    pub count: usize,
    pub down_bps: u64,
    pub up_bps: u64,
}

#[derive(Clone, Debug)]
pub struct TcpSnapshot {
    pub total_count: usize,
    pub total_down_bps: u64,
    pub total_up_bps: u64,
    pub by_ip_port: Vec<IpPortGroup>,
    pub by_ip_process: Vec<IpProcessGroup>,
}

pub fn capture_tcp_snapshot(tracker: &mut SpeedTracker) -> Result<TcpSnapshot, String> {
    let mut connections = Vec::new();
    connections.extend(read_tcp_v4(tracker)?);
    connections.extend(read_tcp_v6(tracker)?);

    let if_octets = read_interface_octets()?;
    let samples: Vec<ConnectionSample> = connections
        .iter()
        .map(|c| ConnectionSample {
            key: c.key.clone(),
            bytes_in: c.bytes_in,
            bytes_out: c.bytes_out,
        })
        .collect();
    let (total_down_bps, total_up_bps, per_conn_speed) = tracker.update(&samples, if_octets);

    let total_count = connections.len();
    let by_ip_port = group_by_ip_port(&connections, &per_conn_speed);
    let by_ip_process = group_by_ip_process(&connections, &per_conn_speed)?;

    Ok(TcpSnapshot {
        total_count,
        total_down_bps,
        total_up_bps,
        by_ip_port,
        by_ip_process,
    })
}

fn read_tcp_v4(tracker: &mut SpeedTracker) -> Result<Vec<TcpConnection>, String> {
    let table = fetch_tcp_table::<MIB_TCPTABLE_OWNER_PID>(AF_INET.0.into())?;
    let rows = unsafe {
        std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
    };

    Ok(rows
        .iter()
        .filter_map(|row| parse_tcp_v4_row(row, tracker))
        .collect())
}

fn read_tcp_v6(tracker: &mut SpeedTracker) -> Result<Vec<TcpConnection>, String> {
    let table = fetch_tcp_table::<MIB_TCP6TABLE_OWNER_PID>(AF_INET6.0.into())?;
    let rows = unsafe {
        std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
    };

    Ok(rows
        .iter()
        .filter_map(|row| parse_tcp_v6_row(row, tracker))
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

fn parse_tcp_v4_row(row: &MIB_TCPROW_OWNER_PID, tracker: &mut SpeedTracker) -> Option<TcpConnection> {
    let remote_ip = ipv4_to_string(row.dwRemoteAddr);
    if is_unspecified_v4(&remote_ip) || row.dwState != MIB_TCP_STATE_ESTAB {
        return None;
    }

    let local_ip = ipv4_to_string(row.dwLocalAddr);
    let local_port = port_from_dw(row.dwLocalPort);
    let remote_port = port_from_dw(row.dwRemotePort);
    let key = ConnKey {
        local_ip,
        local_port,
        remote_ip: remote_ip.clone(),
        remote_port,
        owning_pid: row.dwOwningPid,
        is_ipv6: false,
    };

    let (bytes_in, bytes_out) = read_tcp_v4_stats(row, tracker, &key);

    Some(TcpConnection {
        key,
        remote_ip,
        remote_port,
        owning_pid: row.dwOwningPid,
        bytes_in,
        bytes_out,
    })
}

fn parse_tcp_v6_row(
    row: &MIB_TCP6ROW_OWNER_PID,
    tracker: &mut SpeedTracker,
) -> Option<TcpConnection> {
    let remote_ip = ipv6_to_string(&row.ucRemoteAddr);
    if is_unspecified_v6(&remote_ip) || row.dwState != MIB_TCP_STATE_ESTAB {
        return None;
    }

    let local_ip = ipv6_to_string(&row.ucLocalAddr);
    let local_port = port_from_dw(row.dwLocalPort);
    let remote_port = port_from_dw(row.dwRemotePort);
    let key = ConnKey {
        local_ip,
        local_port,
        remote_ip: remote_ip.clone(),
        remote_port,
        owning_pid: row.dwOwningPid,
        is_ipv6: true,
    };

    let (bytes_in, bytes_out) = read_tcp_v6_stats(row, tracker, &key);

    Some(TcpConnection {
        key,
        remote_ip,
        remote_port,
        owning_pid: row.dwOwningPid,
        bytes_in,
        bytes_out,
    })
}

fn read_tcp_v4_stats(
    row: &MIB_TCPROW_OWNER_PID,
    tracker: &mut SpeedTracker,
    key: &ConnKey,
) -> (u64, u64) {
    let row_lh = MIB_TCPROW_LH {
        Anonymous: MIB_TCPROW_LH_0 { dwState: row.dwState },
        dwLocalAddr: row.dwLocalAddr,
        dwLocalPort: row.dwLocalPort,
        dwRemoteAddr: row.dwRemoteAddr,
        dwRemotePort: row.dwRemotePort,
    };

    if tracker.needs_estats_enable(key) {
        enable_tcp_v4_estats(&row_lh);
        tracker.mark_estats_enabled(key.clone());
    }

    read_estats_v4(&row_lh).unwrap_or((0, 0))
}

fn read_tcp_v6_stats(
    row: &MIB_TCP6ROW_OWNER_PID,
    tracker: &mut SpeedTracker,
    key: &ConnKey,
) -> (u64, u64) {
    let row6 = owner_pid_to_tcp6_row(row);

    if tracker.needs_estats_enable(key) {
        enable_tcp_v6_estats(&row6);
        tracker.mark_estats_enabled(key.clone());
    }

    read_estats_v6(&row6).unwrap_or((0, 0))
}

fn enable_tcp_v4_estats(row: &MIB_TCPROW_LH) {
    let rw = TCP_ESTATS_DATA_RW_v0 { EnableCollection: TRUE };
    let rw_bytes = unsafe {
        std::slice::from_raw_parts(
            (&rw as *const TCP_ESTATS_DATA_RW_v0).cast::<u8>(),
            size_of::<TCP_ESTATS_DATA_RW_v0>(),
        )
    };
    unsafe {
        let _ = SetPerTcpConnectionEStats(row, TcpConnectionEstatsData, rw_bytes, 0, 0);
    }
}

fn enable_tcp_v6_estats(row: &MIB_TCP6ROW) {
    let rw = TCP_ESTATS_DATA_RW_v0 { EnableCollection: TRUE };
    let rw_bytes = unsafe {
        std::slice::from_raw_parts(
            (&rw as *const TCP_ESTATS_DATA_RW_v0).cast::<u8>(),
            size_of::<TCP_ESTATS_DATA_RW_v0>(),
        )
    };
    unsafe {
        let _ = SetPerTcp6ConnectionEStats(row, TcpConnectionEstatsData, rw_bytes, 0, 0);
    }
}

fn read_estats_v4(row: &MIB_TCPROW_LH) -> Option<(u64, u64)> {
    let mut rod = TCP_ESTATS_DATA_ROD_v0::default();
    let rod_bytes = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut rod as *mut TCP_ESTATS_DATA_ROD_v0).cast::<u8>(),
            size_of::<TCP_ESTATS_DATA_ROD_v0>(),
        )
    };
    let ret = unsafe {
        GetPerTcpConnectionEStats(
            row,
            TcpConnectionEstatsData,
            None,
            0,
            None,
            0,
            Some(rod_bytes),
            0,
        )
    };
    if ret != 0 {
        return None;
    }
    Some((rod.DataBytesIn, rod.DataBytesOut))
}

fn read_estats_v6(row: &MIB_TCP6ROW) -> Option<(u64, u64)> {
    let mut rod = TCP_ESTATS_DATA_ROD_v0::default();
    let rod_bytes = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut rod as *mut TCP_ESTATS_DATA_ROD_v0).cast::<u8>(),
            size_of::<TCP_ESTATS_DATA_ROD_v0>(),
        )
    };
    let ret = unsafe {
        GetPerTcp6ConnectionEStats(
            row,
            TcpConnectionEstatsData,
            None,
            0,
            None,
            0,
            Some(rod_bytes),
            0,
        )
    };
    if ret != 0 {
        return None;
    }
    Some((rod.DataBytesIn, rod.DataBytesOut))
}

fn owner_pid_to_tcp6_row(row: &MIB_TCP6ROW_OWNER_PID) -> MIB_TCP6ROW {
    MIB_TCP6ROW {
        State: windows::Win32::NetworkManagement::IpHelper::MIB_TCP_STATE(row.dwState),
        LocalAddr: IN6_ADDR {
            u: windows::Win32::Networking::WinSock::IN6_ADDR_0 {
                Byte: row.ucLocalAddr,
            },
        },
        dwLocalScopeId: row.dwLocalScopeId,
        dwLocalPort: row.dwLocalPort,
        RemoteAddr: IN6_ADDR {
            u: windows::Win32::Networking::WinSock::IN6_ADDR_0 {
                Byte: row.ucRemoteAddr,
            },
        },
        dwRemoteScopeId: row.dwRemoteScopeId,
        dwRemotePort: row.dwRemotePort,
    }
}

pub fn read_interface_octets() -> Result<(u64, u64), String> {
    let mut size = 0u32;
    unsafe {
        let _ = GetIfTable(None, &mut size, false);
    }
    if size == 0 {
        return Ok((0, 0));
    }

    let mut buffer = vec![0u8; size as usize];
    loop {
        let ret = unsafe { GetIfTable(Some(buffer.as_mut_ptr().cast()), &mut size, false) };
        if ret == 0 {
            break;
        }
        if ret == ERROR_INSUFFICIENT_BUFFER.0 {
            buffer.resize(size as usize, 0);
            continue;
        }
        return Err(format!("GetIfTable 失败，错误码: {ret}"));
    }

    let table = unsafe { &*(buffer.as_ptr() as *const MIB_IFTABLE) };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize) };

    let mut in_octets = 0u64;
    let mut out_octets = 0u64;
    for row in rows {
        if row.dwType == IF_TYPE_SOFTWARE_LOOPBACK || row.dwOperStatus.0 != 1 {
            continue;
        }
        in_octets += u64::from(row.dwInOctets);
        out_octets += u64::from(row.dwOutOctets);
    }
    Ok((in_octets, out_octets))
}

fn group_by_ip_port(
    connections: &[TcpConnection],
    speeds: &HashMap<ConnKey, ConnSpeed>,
) -> Vec<IpPortGroup> {
    let mut counts: HashMap<(String, u16), (usize, u64, u64)> = HashMap::new();
    for conn in connections {
        let spd = speeds.get(&conn.key).copied().unwrap_or_default();
        let entry = counts
            .entry((conn.remote_ip.clone(), conn.remote_port))
            .or_default();
        entry.0 += 1;
        entry.1 += spd.bytes_per_sec_in;
        entry.2 += spd.bytes_per_sec_out;
    }

    let mut groups: Vec<IpPortGroup> = counts
        .into_iter()
        .map(|((remote_ip, remote_port), (count, down_bps, up_bps))| IpPortGroup {
            remote_ip,
            remote_port,
            count,
            down_bps,
            up_bps,
        })
        .collect();

    groups.sort_by(|a, b| {
        b.down_bps
            .saturating_add(b.up_bps)
            .cmp(&a.down_bps.saturating_add(a.up_bps))
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.remote_ip.cmp(&b.remote_ip))
            .then_with(|| a.remote_port.cmp(&b.remote_port))
    });
    groups
}

fn group_by_ip_process(
    connections: &[TcpConnection],
    speeds: &HashMap<ConnKey, ConnSpeed>,
) -> Result<Vec<IpProcessGroup>, String> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let mut counts: HashMap<(String, String), (usize, u64, u64)> = HashMap::new();
    for conn in connections {
        let process_name = resolve_process_name(&system, conn.owning_pid);
        let spd = speeds.get(&conn.key).copied().unwrap_or_default();
        let entry = counts
            .entry((conn.remote_ip.clone(), process_name))
            .or_default();
        entry.0 += 1;
        entry.1 += spd.bytes_per_sec_in;
        entry.2 += spd.bytes_per_sec_out;
    }

    let mut groups: Vec<IpProcessGroup> = counts
        .into_iter()
        .map(|((remote_ip, process_name), (count, down_bps, up_bps))| IpProcessGroup {
            remote_ip,
            process_name,
            count,
            down_bps,
            up_bps,
        })
        .collect();

    groups.sort_by(|a, b| {
        b.down_bps
            .saturating_add(b.up_bps)
            .cmp(&a.down_bps.saturating_add(a.up_bps))
            .then_with(|| b.count.cmp(&a.count))
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

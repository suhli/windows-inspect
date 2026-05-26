use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use ferrisetw::parser::Parser;
use ferrisetw::provider::{kernel_providers, Provider};
use ferrisetw::schema_locator::SchemaLocator;
use ferrisetw::trace::KernelTrace;
use ferrisetw::EventRecord;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TrafficKey {
    pub remote_ip: String,
    pub remote_port: u16,
    pub pid: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TrafficCounters {
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Clone, Debug, Default)]
pub struct EtwTrafficSnapshot {
    pub counters: HashMap<TrafficKey, TrafficCounters>,
}

pub struct EtwTrafficCollector {
    counters: Arc<Mutex<HashMap<TrafficKey, TrafficCounters>>>,
    _trace: KernelTrace,
}

impl EtwTrafficCollector {
    pub fn start() -> Result<Self, String> {
        let counters = Arc::new(Mutex::new(HashMap::new()));
        let callback_counters = counters.clone();

        let callback = move |record: &EventRecord, schema_locator: &SchemaLocator| {
            handle_tcpip_event(record, schema_locator, &callback_counters);
        };

        let provider = Provider::kernel(&kernel_providers::TCP_IP_PROVIDER)
            .add_callback(callback)
            .build();

        let trace = KernelTrace::new()
            .named("windows-inspect-etw-network".to_string())
            .enable(provider)
            .start_and_process()
            .map_err(|err| format!("启动 ETW 网络采集失败: {err:?}"))?;

        Ok(Self {
            counters,
            _trace: trace,
        })
    }

    pub fn snapshot(&self) -> EtwTrafficSnapshot {
        let counters = self
            .counters
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        EtwTrafficSnapshot { counters }
    }
}

fn handle_tcpip_event(
    record: &EventRecord,
    schema_locator: &SchemaLocator,
    counters: &Arc<Mutex<HashMap<TrafficKey, TrafficCounters>>>,
) {
    let opcode = record.opcode();
    let event_id = record.event_id();
    let is_send = matches!(opcode, 10 | 26) || matches!(event_id, 10 | 26);
    let is_recv = matches!(opcode, 11 | 27) || matches!(event_id, 11 | 27);
    if !is_send && !is_recv {
        return;
    }

    let Ok(schema) = schema_locator.event_schema(record) else {
        return;
    };
    let parser = Parser::create(record, &schema);

    let pid = parse_u32(&parser, &["PID", "ProcessId"]).unwrap_or(record.process_id());
    let size = parse_u32(&parser, &["size", "Size"]).unwrap_or(0);
    if pid == 0 || size == 0 {
        return;
    }

    let ip_names = if is_send {
        ["daddr", "DAddr", "DestinationAddress"]
    } else {
        ["saddr", "SAddr", "SourceAddress"]
    };
    let port_names = if is_send {
        ["dport", "DPort", "DestinationPort"]
    } else {
        ["sport", "SPort", "SourcePort"]
    };

    let remote_ips = parse_ips(&parser, &ip_names);
    if remote_ips.is_empty() {
        return;
    }
    let remote_ports = parse_ports(&parser, &port_names);
    if remote_ports.is_empty() {
        return;
    }

    if let Ok(mut guard) = counters.lock() {
        for remote_ip in &remote_ips {
            for &remote_port in &remote_ports {
                let key = TrafficKey {
                    remote_ip: remote_ip.to_string(),
                    remote_port,
                    pid,
                };
                let entry = guard.entry(key).or_default();
                if is_send {
                    entry.bytes_out = entry.bytes_out.saturating_add(size as u64);
                } else {
                    entry.bytes_in = entry.bytes_in.saturating_add(size as u64);
                }
            }
        }
    }
}

fn parse_u32(parser: &Parser<'_, '_>, names: &[&str]) -> Option<u32> {
    for name in names {
        if let Ok(value) = parser.try_parse::<u32>(name) {
            return Some(value);
        }
    }
    None
}

fn parse_ips(parser: &Parser<'_, '_>, names: &[&str]) -> Vec<IpAddr> {
    let mut ips = Vec::new();
    for name in names {
        if let Ok(value) = parser.try_parse::<IpAddr>(name) {
            push_ip_variants(&mut ips, value);
        }
        if let Ok(value) = parser.try_parse::<u32>(name) {
            push_ip_variants(&mut ips, IpAddr::V4(value.to_be().into()));
        }
    }
    ips
}

fn parse_ports(parser: &Parser<'_, '_>, names: &[&str]) -> Vec<u16> {
    let mut ports = Vec::new();
    for name in names {
        if let Ok(value) = parser.try_parse::<u16>(name) {
            push_port_variant(&mut ports, value);
            push_port_variant(&mut ports, u16::from_be(value));
        }
        if let Ok(value) = parser.try_parse::<u32>(name) {
            let port = (value & 0xFFFF) as u16;
            push_port_variant(&mut ports, port);
            push_port_variant(&mut ports, u16::from_be(port));
        }
    }
    ports
}

fn push_ip_variants(ips: &mut Vec<IpAddr>, ip: IpAddr) {
    push_ip_variant(ips, ip);
    if let IpAddr::V4(v4) = ip {
        push_ip_variant(ips, IpAddr::V4(u32::from(v4).swap_bytes().into()));
    }
}

fn push_ip_variant(ips: &mut Vec<IpAddr>, ip: IpAddr) {
    if !ips.contains(&ip) {
        ips.push(ip);
    }
}

fn push_port_variant(ports: &mut Vec<u16>, port: u16) {
    if port != 0 && !ports.contains(&port) {
        ports.push(port);
    }
}

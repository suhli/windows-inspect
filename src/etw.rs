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
    let is_send = matches!(opcode, 10 | 26);
    let is_recv = matches!(opcode, 11 | 27);
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

    let Some(remote_ip) = parse_ip(&parser, &ip_names) else {
        return;
    };
    let Some(remote_port) = parse_port(&parser, &port_names) else {
        return;
    };

    let key = TrafficKey {
        remote_ip: remote_ip.to_string(),
        remote_port,
        pid,
    };

    if let Ok(mut guard) = counters.lock() {
        let entry = guard.entry(key).or_default();
        if is_send {
            entry.bytes_out = entry.bytes_out.saturating_add(size as u64);
        } else {
            entry.bytes_in = entry.bytes_in.saturating_add(size as u64);
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

fn parse_ip(parser: &Parser<'_, '_>, names: &[&str]) -> Option<IpAddr> {
    for name in names {
        if let Ok(value) = parser.try_parse::<IpAddr>(name) {
            return Some(value);
        }
    }
    None
}

fn parse_port(parser: &Parser<'_, '_>, names: &[&str]) -> Option<u16> {
    for name in names {
        if let Ok(value) = parser.try_parse::<u16>(name) {
            return Some(value);
        }
        if let Ok(value) = parser.try_parse::<u32>(name) {
            if value <= u16::MAX as u32 {
                return Some(value as u16);
            }
            return Some(u16::from_be((value & 0xFFFF) as u16));
        }
    }
    None
}

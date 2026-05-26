use std::collections::HashMap;
use std::time::Instant;

const HISTORY_LEN: usize = 60;
/// 单次采样间隔内，单连接允许计入的最大字节增量（约 500 MB/s × 2s）
const MAX_CONN_DELTA_PER_SAMPLE: u64 = 1_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConnKey {
    pub local_ip: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,
    pub owning_pid: u32,
    pub is_ipv6: bool,
}

#[derive(Clone, Debug)]
pub struct ConnectionSample {
    pub key: ConnKey,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct ByteSample {
    bytes_in: u64,
    bytes_out: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ConnSpeed {
    pub bytes_per_sec_in: u64,
    pub bytes_per_sec_out: u64,
}

pub struct SpeedTracker {
    last_poll: Option<Instant>,
    conn_samples: HashMap<ConnKey, ByteSample>,
    if_counters: HashMap<u32, (u32, u32)>,
    estats_enabled: HashMap<ConnKey, ()>,
    download_history: Vec<f32>,
    upload_history: Vec<f32>,
}

impl SpeedTracker {
    pub fn new() -> Self {
        Self {
            last_poll: None,
            conn_samples: HashMap::new(),
            if_counters: HashMap::new(),
            estats_enabled: HashMap::new(),
            download_history: Vec::with_capacity(HISTORY_LEN),
            upload_history: Vec::with_capacity(HISTORY_LEN),
        }
    }

    pub fn download_history(&self) -> &[f32] {
        &self.download_history
    }

    pub fn upload_history(&self) -> &[f32] {
        &self.upload_history
    }

    pub fn update(
        &mut self,
        connections: &[ConnectionSample],
        if_counters: HashMap<u32, (u32, u32)>,
    ) -> (u64, u64, HashMap<ConnKey, ConnSpeed>) {
        let now = Instant::now();
        let mut per_conn = HashMap::new();
        let mut total_down = 0u64;
        let mut total_up = 0u64;

        if let Some(prev_time) = self.last_poll {
            let elapsed = now.duration_since(prev_time).as_secs_f64();
            if elapsed > 0.05 {
                for conn in connections {
                    let key = conn.key.clone();
                    let current = ByteSample {
                        bytes_in: conn.bytes_in,
                        bytes_out: conn.bytes_out,
                    };

                    if let Some(prev) = self.conn_samples.get(&key) {
                        if prev.bytes_in > 0 || prev.bytes_out > 0 {
                            let din = counter_delta(prev.bytes_in, current.bytes_in);
                            let dout = counter_delta(prev.bytes_out, current.bytes_out);
                            let spd_in = ((din as f64 / elapsed) as u64).min(MAX_CONN_DELTA_PER_SAMPLE);
                            let spd_out =
                                ((dout as f64 / elapsed) as u64).min(MAX_CONN_DELTA_PER_SAMPLE);
                            per_conn.insert(
                                key.clone(),
                                ConnSpeed {
                                    bytes_per_sec_in: spd_in,
                                    bytes_per_sec_out: spd_out,
                                },
                            );
                        }
                    }

                    self.conn_samples.insert(key, current);
                }

                self.conn_samples
                    .retain(|k, _| connections.iter().any(|c| c.key == *k));

                let (if_down, if_up) = interface_speed(&self.if_counters, &if_counters, elapsed);
                total_down = if_down;
                total_up = if_up;
                self.push_history(if_down as f32, if_up as f32);
            }
        } else {
            for conn in connections {
                self.conn_samples.insert(
                    conn.key.clone(),
                    ByteSample {
                        bytes_in: conn.bytes_in,
                        bytes_out: conn.bytes_out,
                    },
                );
            }
        }

        self.last_poll = Some(now);
        self.if_counters = if_counters;
        (total_down, total_up, per_conn)
    }

    pub fn mark_estats_enabled(&mut self, key: ConnKey) {
        self.estats_enabled.insert(key, ());
    }

    pub fn needs_estats_enable(&self, key: &ConnKey) -> bool {
        !self.estats_enabled.contains_key(key)
    }

    fn push_history(&mut self, down: f32, up: f32) {
        if self.download_history.len() >= HISTORY_LEN {
            self.download_history.remove(0);
            self.upload_history.remove(0);
        }
        self.download_history.push(down);
        self.upload_history.push(up);
    }
}

fn counter_delta(prev: u64, curr: u64) -> u64 {
    if curr >= prev {
        curr - prev
    } else {
        // 计数器重置或回绕：忽略本次增量
        0
    }
}

fn interface_speed(
    prev: &HashMap<u32, (u32, u32)>,
    curr: &HashMap<u32, (u32, u32)>,
    elapsed: f64,
) -> (u64, u64) {
    let mut down_bytes = 0u64;
    let mut up_bytes = 0u64;

    for (index, (cin, cout)) in curr {
        if let Some((pin, pout)) = prev.get(index) {
            down_bytes += cin.wrapping_sub(*pin) as u64;
            up_bytes += cout.wrapping_sub(*pout) as u64;
        }
    }

    (
        (down_bytes as f64 / elapsed) as u64,
        (up_bytes as f64 / elapsed) as u64,
    )
}

pub fn format_speed(bytes_per_sec: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bps = bytes_per_sec as f64;
    if bps >= GB {
        format!("{:.2} GB/s", bps / GB)
    } else if bps >= MB {
        format!("{:.2} MB/s", bps / MB)
    } else if bps >= KB {
        format!("{:.1} KB/s", bps / KB)
    } else {
        format!("{bytes_per_sec} B/s")
    }
}

pub fn build_line_path(samples: &[f32], width: f32, height: f32) -> String {
    if samples.is_empty() || width <= 1.0 || height <= 1.0 {
        return String::new();
    }

    let max = samples
        .iter()
        .copied()
        .fold(0.0f32, f32::max)
        .max(1.0);
    let pad = 4.0;
    let plot_h = height - pad * 2.0;
    let plot_w = width - pad * 2.0;
    let n = samples.len();

    let mut path = String::new();
    for (i, &value) in samples.iter().enumerate() {
        let x = pad
            + if n <= 1 {
                0.0
            } else {
                i as f32 / (n - 1) as f32 * plot_w
            };
        let y = pad + plot_h - (value / max) * plot_h;
        if i == 0 {
            path.push_str(&format!("M {x:.1} {y:.1}"));
        } else {
            path.push_str(&format!(" L {x:.1} {y:.1}"));
        }
    }
    path
}

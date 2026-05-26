#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod net;
mod speed;
mod etw;
mod titlebar;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use net::{build_line_path, capture_tcp_snapshot, format_speed};
use speed::SpeedTracker;
use slint::{ComponentHandle, ModelRc, VecModel};

slint::include_modules!();

const CHART_HEIGHT: f32 = 120.0;
const APP_TITLE: &str = "Windows 网络连接与流量监控";

fn chart_width(ui: &MainWindow) -> f32 {
    let window = ui.window();
    let size = window.size();
    let scale = window.scale_factor();
    let logical_w = size.width as f32 / scale;
    (logical_w - 40.0).max(320.0)
}

fn apply_snapshot(
    ui: &MainWindow,
    snapshot: net::TcpSnapshot,
    tracker: &SpeedTracker,
    etw_status: Option<&str>,
) {
    let chart_w = chart_width(ui);
    let ip_port_count = snapshot.by_ip_port.len();
    let ip_process_count = snapshot.by_ip_process.len();
    let ip_port_process_count = snapshot.by_ip_port_process.len();
    let total = snapshot.total_count;

    let ip_port_model: Rc<VecModel<IpPortRow>> = Rc::new(VecModel::default());
    for group in snapshot.by_ip_port {
        ip_port_model.push(IpPortRow {
            remote_ip: group.remote_ip.into(),
            remote_port: group.remote_port as i32,
            count: group.count as i32,
            down_speed: format_speed(group.down_bps).into(),
            up_speed: format_speed(group.up_bps).into(),
        });
    }

    let ip_process_model: Rc<VecModel<IpProcessRow>> = Rc::new(VecModel::default());
    for group in snapshot.by_ip_process {
        ip_process_model.push(IpProcessRow {
            remote_ip: group.remote_ip.into(),
            process_name: group.process_name.into(),
            count: group.count as i32,
            down_speed: format_speed(group.down_bps).into(),
            up_speed: format_speed(group.up_bps).into(),
        });
    }

    let ip_port_process_model: Rc<VecModel<IpPortProcessRow>> = Rc::new(VecModel::default());
    for group in snapshot.by_ip_port_process {
        ip_port_process_model.push(IpPortProcessRow {
            remote_ip: group.remote_ip.into(),
            remote_port: group.remote_port as i32,
            process_name: group.process_name.into(),
            count: group.count as i32,
            down_speed: format_speed(group.down_bps).into(),
            up_speed: format_speed(group.up_bps).into(),
        });
    }

    ui.set_total_count(total as i32);
    ui.set_total_down_speed(format_speed(snapshot.total_down_bps).into());
    ui.set_total_up_speed(format_speed(snapshot.total_up_bps).into());
    ui.set_ip_port_rows(ModelRc::from(ip_port_model));
    ui.set_ip_process_rows(ModelRc::from(ip_process_model));
    ui.set_ip_port_process_rows(ModelRc::from(ip_port_process_model));
    ui.set_download_path(
        build_line_path(tracker.download_history(), chart_w, CHART_HEIGHT).into(),
    );
    ui.set_upload_path(
        build_line_path(tracker.upload_history(), chart_w, CHART_HEIGHT).into(),
    );
    let etw_status = etw_status.unwrap_or("ETW 网络事件采集中");
    ui.set_status_text(
        format!(
            "已采集 {total} 条 ESTABLISHED 连接 · {ip_port_count} 个 IP+端口组 · {ip_process_count} 个 IP+进程组 · {ip_port_process_count} 个 IP+端口+进程组 · {etw_status}"
        )
        .into(),
    );
    ui.set_last_updated(format_time_now().into());
}

fn refresh_ui(
    ui: &MainWindow,
    tracker: &mut SpeedTracker,
    etw: Option<&etw::EtwTrafficCollector>,
    etw_status: Option<&str>,
) {
    let etw_snapshot = etw
        .map(|collector| collector.snapshot())
        .unwrap_or_default();

    match capture_tcp_snapshot(tracker, &etw_snapshot) {
        Ok(snapshot) => apply_snapshot(ui, snapshot, tracker, etw_status),
        Err(err) => ui.set_status_text(format!("采集失败: {err}").into()),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    let tracker = Rc::new(RefCell::new(SpeedTracker::new()));
    let etw_collector = Rc::new(match etw::EtwTrafficCollector::start() {
        Ok(collector) => Some(collector),
        Err(err) => {
            ui.set_status_text(format!("{err}；请尝试以管理员身份运行").into());
            None
        }
    });
    let etw_status = if etw_collector.is_some() {
        None
    } else {
        Some("ETW 未启动，表格分组速度不可用")
    };

    refresh_ui(
        &ui,
        &mut tracker.borrow_mut(),
        etw_collector.as_ref().as_ref(),
        etw_status,
    );

    let ui_weak = ui.as_weak();
    let tracker_refresh = tracker.clone();
    let etw_refresh = etw_collector.clone();
    ui.on_refresh_requested(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        ui.set_status_text("正在刷新…".into());
        let status = if etw_refresh.is_some() {
            None
        } else {
            Some("ETW 未启动，表格分组速度不可用")
        };
        refresh_ui(
            &ui,
            &mut tracker_refresh.borrow_mut(),
            etw_refresh.as_ref().as_ref(),
            status,
        );
    });

    let ui_weak = ui.as_weak();
    let tracker_timer = tracker.clone();
    let etw_timer = etw_collector.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(2),
        move || {
            if let Some(ui) = ui_weak.upgrade() {
                let status = if etw_timer.is_some() {
                    None
                } else {
                    Some("ETW 未启动，表格分组速度不可用")
                };
                refresh_ui(
                    &ui,
                    &mut tracker_timer.borrow_mut(),
                    etw_timer.as_ref().as_ref(),
                    status,
                );
            }
        },
    );

    ui.show()?;
    titlebar::apply_native_style(APP_TITLE);
    slint::run_event_loop()?;
    Ok(())
}

fn format_time_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let total_secs = secs % 86_400;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

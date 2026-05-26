#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod net;

use std::rc::Rc;
use std::time::Duration;

use slint::{ModelRc, VecModel};

slint::include_modules!();

fn apply_snapshot(ui: &MainWindow, snapshot: net::TcpSnapshot) {
    let ip_port_count = snapshot.by_ip_port.len();
    let ip_process_count = snapshot.by_ip_process.len();
    let total = snapshot.total_count;

    let ip_port_model: Rc<VecModel<IpPortRow>> = Rc::new(VecModel::default());
    for group in snapshot.by_ip_port {
        ip_port_model.push(IpPortRow {
            remote_ip: group.remote_ip.into(),
            remote_port: group.remote_port as i32,
            count: group.count as i32,
        });
    }

    let ip_process_model: Rc<VecModel<IpProcessRow>> = Rc::new(VecModel::default());
    for group in snapshot.by_ip_process {
        ip_process_model.push(IpProcessRow {
            remote_ip: group.remote_ip.into(),
            process_name: group.process_name.into(),
            count: group.count as i32,
        });
    }

    ui.set_total_count(total as i32);
    ui.set_ip_port_rows(ModelRc::from(ip_port_model));
    ui.set_ip_process_rows(ModelRc::from(ip_process_model));
    ui.set_status_text(
        format!(
            "已采集 {total} 条 TCP 连接 · {ip_port_count} 个 IP+端口组 · {ip_process_count} 个 IP+进程组"
        )
        .into(),
    );
    ui.set_last_updated(format_time_now().into());
}

fn refresh_ui(ui: &MainWindow) {
    match net::capture_tcp_snapshot() {
        Ok(snapshot) => apply_snapshot(ui, snapshot),
        Err(err) => ui.set_status_text(format!("采集失败: {err}").into()),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;

    refresh_ui(&ui);

    let ui_weak = ui.as_weak();
    ui.on_refresh_requested(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        ui.set_status_text("正在刷新…".into());
        refresh_ui(&ui);
    });

    let ui_weak = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(3),
        move || {
            if let Some(ui) = ui_weak.upgrade() {
                refresh_ui(&ui);
            }
        },
    );

    ui.run()?;
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

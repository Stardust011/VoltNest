//! 桌面应用入口。启动顺序：单实例互斥 → 数据目录/日志 → AUMID → 设置/状态（损坏不阻塞）→ Tauri/托盘/宿主/Provider。
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]
#![deny(clippy::disallowed_types)]

mod commands;
mod devices;
mod dto;
mod floating;
mod host;
mod logger;
mod notification;
mod panel;
mod probe;
mod supervisor;
mod toast_icon;
mod tray;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use tauri::{Manager, RunEvent, WindowEvent};

use voltnest_core::DiagLevel;
use voltnest_platform::single_instance::{Instance, acquire};
use voltnest_provider::clock;
use voltnest_storage::{DataPaths, SettingsStore, StateStore};

use crate::host::{HostState, lock};
use crate::supervisor::{SupervisorCmd, TransportSpec};

fn main() {
    // 悬浮窗子进程：同一个 exe 的另一种人格，跑纯 Slint 会话。
    // 必须在下面的单实例互斥之前分流，否则子进程会把自己判成 Second 直接 return，
    // 表现为「窗口一闪而过」。
    if std::env::args().any(|arg| arg == floating::HOST_FLAG) {
        std::process::exit(run_floating_host());
    }

    let guard = match acquire() {
        Instance::Second => return,
        Instance::First(guard) => Some(guard),
        Instance::Unavailable(reason) => {
            eprintln!("单实例互斥体不可用，继续启动：{reason}");
            None
        }
    };

    let launched_by_autostart =
        std::env::args().any(|arg| arg == voltnest_platform::AUTOSTART_FLAG);

    let paths = DataPaths::resolve();
    if let Err(err) = paths.ensure_root() {
        eprintln!("创建数据目录失败：{err}");
    }
    logger::init(&paths.logs_dir());
    log::info!("VoltNest 启动（autostart={launched_by_autostart}）");

    let toast_icon = toast_icon::init(paths.root());
    if let Err(err) = voltnest_platform::aumid::register("VoltNest", toast_icon.as_deref()) {
        log::warn!("AUMID 注册失败：{err}");
    }

    let settings_store = SettingsStore::new(paths.clone());
    let loaded = settings_store.load(clock::now_utc_ms());
    let state_store = StateStore::new(paths.clone());

    let (tx, rx) = mpsc::channel();
    let shared = HostState::new(
        paths.clone(),
        loaded.settings.clone(),
        settings_store,
        state_store,
        tx,
    );
    shared.apply_settings(&loaded.settings);

    if let Some(warning) = loaded.warning {
        let detail = warning.to_string();
        log::warn!("设置加载告警：{detail}");
        lock(&shared.engine).push_diag(
            clock::now_utc_ms(),
            DiagLevel::Warn,
            "settings_load_warning",
            None,
            detail,
        );
    }
    if loaded.settings.autostart {
        let actual = voltnest_platform::autostart::is_enabled();
        if !actual {
            let mut next = lock(&shared.settings).clone();
            next.autostart = false;
            if let Err(err) = shared.settings_store.save(&next) {
                log::warn!("回写 autostart 状态失败：{err}");
            }
            *lock(&shared.settings) = next;
        }
    }

    let start_minimized = loaded.settings.start_minimized;
    let shared_for_exit = Arc::clone(&shared);

    let probe_state = Arc::new(probe::ProbeState::new(paths.clone(), clock::now_utc_ms()));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(Arc::clone(&shared))
        .manage(probe_state)
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_settings,
            commands::update_settings,
            commands::get_diagnostics,
            commands::restart_transport,
            commands::test_notification,
            commands::set_autostart,
            commands::complete_onboarding,
            commands::show_panel,
            commands::hide_to_tray,
            commands::quit_app,
            devices::list_device_candidates,
            devices::add_device,
            devices::remove_devices,
            devices::merge_devices,
            devices::separate_merge,
            devices::rename_device,
            probe::list_hid_targets,
            probe::start_hid_capture,
            probe::stop_hid_capture,
            probe::discard_hid_capture,
            probe::generate_issue_report,
            probe::save_manual_device,
            probe::list_manual_devices,
            probe::remove_manual_device,
            probe::get_manual_report,
            floating::get_floating_units,
            floating::set_floating_visible,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            if let Err(err) = tray::install(&handle) {
                log::error!("托盘初始化失败：{err}");
            }

            host::spawn_host_loop(handle.clone(), Arc::clone(&shared), rx);

            let mut supervisors = HashMap::new();
            for spec in [TransportSpec::hid(), TransportSpec::ble()] {
                let transport = spec.transport;
                let sender = supervisor::spawn_supervisor(Arc::clone(&shared), spec);
                supervisors.insert(transport, sender);
            }
            *lock(&shared.supervisors) = supervisors;

            host::publish_snapshot(&handle, &shared, clock::now_utc_ms());

            if let Some(guard) = guard {
                let stop = Arc::new(AtomicBool::new(false));
                let activation_handle = handle.clone();
                let _ = guard.listen_activation(stop, move || {
                    panel::show(&activation_handle);
                });
                app.manage(guard);
            }

            if start_minimized {
                panel::hide(&handle);
            } else {
                panel::show(&handle);
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                panel::hide(window.app_handle());
                let shared = window.app_handle().state::<Arc<HostState>>();
                host::persist_state(&shared, clock::now_utc_ms(), true);
            }
        })
        .build(tauri::generate_context!())
        .expect("VoltNest 初始化失败");

    app.run(move |app, event| match event {
        // 主窗口可能在隐藏满 1 分钟后被回收，窗口全关不等于退出：进程继续留在托盘。
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        RunEvent::Exit => {
            log::info!("VoltNest 退出");
            floating::shutdown(&shared_for_exit);
            shared_for_exit
                .shutting_down
                .store(true, std::sync::atomic::Ordering::Relaxed);
            for sender in lock(&shared_for_exit.supervisors).values() {
                let _ = sender.send(SupervisorCmd::Stop);
            }
            host::persist_state(&shared_for_exit, clock::now_utc_ms(), true);
            let _ = app;
        }
        _ => {}
    });
}

/// 子进程人格：跑悬浮窗会话直到 stdin 关闭。
///
/// 不装文件日志（会和父进程抢同一个 append 句柄）、不注册 AUMID、不碰 autostart。
fn run_floating_host() -> i32 {
    // panic 也要让父进程知道，否则它只会看到 EOF 而盲目重启。
    std::panic::set_hook(Box::new(|info| {
        let _ = std::io::Write::write_all(
            &mut std::io::stdout(),
            format!(
                r#"{{"t":"bye","v":{},"reason":"panic: {info}"}}"#,
                voltnest_floating::PROTOCOL_VERSION
            )
            .as_bytes(),
        );
        let _ = std::io::Write::flush(&mut std::io::stdout());
        std::process::exit(1);
    }));

    let options = voltnest_floating::SessionOptions {
        mode: voltnest_floating::SessionMode::Hosted,
    };
    match voltnest_floating::run_session(options) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}

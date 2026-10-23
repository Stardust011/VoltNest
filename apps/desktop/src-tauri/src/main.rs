//! 桌面应用入口。启动顺序不可调换：单实例互斥 → 数据目录/日志 → AUMID → 设置/状态（损坏不阻塞）→ Tauri/托盘/宿主/Provider。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(clippy::disallowed_types)]

mod commands;
mod dto;
mod host;
mod logger;
mod notification;
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

use crate::host::{Shared, lock};
use crate::supervisor::{Spec, SupervisorCmd};

fn main() {
    // 1. 单实例：第二个进程给首实例发信号后立刻退出，绝不创建窗口。
    let guard = match acquire() {
        Instance::Second => return,
        Instance::First(guard) => Some(guard),
        // 权限异常时不阻塞启动：宁可多开一个窗口，也不要打不开。
        Instance::Unavailable(reason) => {
            eprintln!("单实例互斥体不可用，继续启动：{reason}");
            None
        }
    };

    let launched_by_autostart =
        std::env::args().any(|arg| arg == voltnest_platform::AUTOSTART_FLAG);

    // 2. 数据目录与日志。
    let paths = DataPaths::resolve();
    if let Err(err) = paths.ensure_root() {
        eprintln!("创建数据目录失败：{err}");
    }
    logger::init(&paths.logs_dir());
    log::info!("VoltNest 启动（autostart={launched_by_autostart}）");

    // 3. AUMID 先于首次 Toast 就位；图标须先落盘，`IconUri` 只认真实文件。
    let toast_icon = toast_icon::init(paths.root());
    if let Err(err) = voltnest_platform::aumid::register("VoltNest", toast_icon.as_deref()) {
        log::warn!("AUMID 注册失败：{err}");
    }

    // 4. 设置与状态。
    let settings_store = SettingsStore::new(paths.clone());
    let loaded = settings_store.load(clock::now_utc_ms());
    let state_store = StateStore::new(paths.clone());

    let (tx, rx) = mpsc::channel();
    let shared = Shared::new(
        paths.clone(),
        loaded.settings.clone(),
        settings_store,
        state_store,
        tx,
    );
    shared.apply_settings(&loaded.settings);

    // 设置损坏 / 版本过高：备份 + 默认值启动 + 诊断告警，不阻塞。
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
    // 自启项可能是用户手动删掉的：以注册表为准回写设置，界面才不会说谎。
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

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(Arc::clone(&shared))
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
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // 5.1 托盘（基础图标 + 菜单），失败不阻塞窗口。
            if let Err(err) = tray::install(&handle) {
                log::error!("托盘初始化失败：{err}");
            }

            // 5.2 宿主事件循环：唯一持有 Engine 的地方。
            host::spawn_loop(handle.clone(), Arc::clone(&shared), rx);

            // 5.3 Provider 线程组监管。
            let mut supervisors = HashMap::new();
            for spec in [Spec::hid(), Spec::ble()] {
                let transport = spec.transport;
                let sender = supervisor::start(Arc::clone(&shared), spec);
                supervisors.insert(transport, sender);
            }
            *lock(&shared.supervisors) = supervisors;

            // 5.4 首帧快照：窗口显示时就已经有数据，不会先闪一下空列表。
            host::publish(&handle, &shared, clock::now_utc_ms());

            // 5.5 二次启动唤醒。
            if let Some(guard) = guard {
                let stop = Arc::new(AtomicBool::new(false));
                let activation_handle = handle.clone();
                let _ = guard.listen_activation(stop, move || {
                    tray::show_main(&activation_handle);
                });
                // 守卫必须活到进程结束：交给 Tauri 托管，随 App 一起释放。
                app.manage(guard);
            }

            // 5.6 首屏显隐：自启与手动启动一致，统一由「启动时最小化到托盘」管控。
            if let Some(window) = app.get_webview_window("main")
                && !start_minimized
            {
                let _ = window.show();
                let _ = window.set_focus();
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // 关闭窗口只隐藏到托盘；退出走托盘菜单。
                api.prevent_close();
                let _ = window.hide();
                let shared = window.app_handle().state::<Arc<Shared>>();
                host::persist(&shared, clock::now_utc_ms(), true);
            }
        })
        .build(tauri::generate_context!())
        .expect("VoltNest 初始化失败");

    app.run(move |app, event| {
        if let RunEvent::Exit = event {
            log::info!("VoltNest 退出");
            shared_for_exit
                .shutting_down
                .store(true, std::sync::atomic::Ordering::Relaxed);
            // 通知全部监管线程停止，让 provider 释放句柄（GATT 订阅、HID 句柄）。
            for sender in lock(&shared_for_exit.supervisors).values() {
                let _ = sender.send(SupervisorCmd::Stop);
            }
            // 退出前强制落盘，别让最后 5s 内的电量变化消失。
            host::persist(&shared_for_exit, clock::now_utc_ms(), true);
            let _ = app;
        }
    });
}

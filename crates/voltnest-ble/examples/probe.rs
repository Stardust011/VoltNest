//! 诊断探针：确认本机 BLE 后端的两条枚举路径，并做一次真实读取。
//!
//! 运行：`cargo run -p voltnest-ble --example probe`
//!
//! Windows（WinRT）：`peripherals()` 只返回本次会话扫描到的广播设备；
//! 已连接设备要靠 `retrieve_peripherals()` 的「已连接 + 含 0x180F」选择器。
//!
//! Linux（bluez）：`peripherals()` 直接查 bluetoothd 的 ObjectManager，返回全部已知设备（含未连接）；
//! `retrieve_peripherals(services=[0x180F])` 只返回「已连接且 Device1.UUIDs 已解析出 0x180F」的设备，
//! 而 UUIDs 在服务解析前是空的 —— 这正是 provider 侧不把服务 UUID 当选择器的原因。
//!
//! 探针**永不 panic**：没有适配器 / bluetoothd 未运行 / 权限不足时打印诊断结论并正常退出，
//! 因此它既能当真机探针，也能在 CI 上验证「无硬件时优雅降级」。

use btleplug::api::{Central, Manager as _, Peripheral as _, RetrievePeripheralsOptions};
use btleplug::platform::{Manager, Peripheral};
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

fn uuid16(short: u16) -> Uuid {
    Uuid::from_u128(u128::from(short) << 96 | 0x0000_1000_8000_0080_5f9b_34fb)
}

/// 平台前提：探针结论只有在知道「枚举从哪来」时才可读。
#[cfg(target_os = "linux")]
const PLATFORM_NOTE: &str = "平台：Linux/bluez —— 需 bluetoothd 运行且系统 dbus 可用；当前用户需在 bluetooth 组或有 polkit 授权";

/// 平台前提：探针结论只有在知道「枚举从哪来」时才可读。
#[cfg(target_os = "windows")]
const PLATFORM_NOTE: &str = "平台：Windows/WinRT —— peripherals() 只回放本次扫描的广播；已连接设备走 retrieve_peripherals 选择器";

/// 平台前提：探针结论只有在知道「枚举从哪来」时才可读。
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const PLATFORM_NOTE: &str = "平台：非 Windows/Linux —— 枚举能力取决于 btleplug 后端实现";

#[tokio::main]
async fn main() {
    println!("{PLATFORM_NOTE}");

    let manager = match Manager::new().await {
        Ok(manager) => manager,
        Err(e) => {
            unavailable(&format!("Manager::new() 失败: {e}"));
            return;
        }
    };
    let adapters = match manager.adapters().await {
        Ok(adapters) => adapters,
        Err(e) => {
            unavailable(&format!("manager.adapters() 失败: {e}"));
            return;
        }
    };
    let Some(adapter) = adapters.first() else {
        unavailable("manager.adapters() 返回空列表（无蓝牙适配器）");
        return;
    };
    println!("adapter_info  = {:?}", adapter.adapter_info().await);
    println!("adapter_state = {:?}", adapter.adapter_state().await);

    match adapter.peripherals().await {
        Ok(plain) => {
            println!("\n[A] Adapter::peripherals() -> {} 台", plain.len());
            for p in &plain {
                println!("    {:?} connected={:?}", p.id(), p.is_connected().await);
            }
        }
        Err(e) => println!("\n[A] Adapter::peripherals() 失败: {e}"),
    }

    let battery = uuid16(0x180F);
    match adapter
        .retrieve_peripherals(RetrievePeripheralsOptions {
            identifiers: None,
            services: Some(vec![battery]),
        })
        .await
    {
        Ok(retrieved) => {
            println!(
                "\n[B] retrieve_peripherals(services=[0x180F]) -> {} 台",
                retrieved.len()
            );
            for p in &retrieved {
                println!("    {:?} connected={:?}", p.id(), p.is_connected().await);
            }
            for p in retrieved {
                probe_device(&p).await;
            }
        }
        Err(e) => println!("\n[B] retrieve_peripherals 失败: {e}"),
    }

    #[cfg(target_os = "linux")]
    match adapter
        .retrieve_peripherals(RetrievePeripheralsOptions::default())
        .await
    {
        Ok(all) => {
            println!("\n[C] retrieve_peripherals(全量) -> {} 台", all.len());
            for p in &all {
                let uuids = p
                    .properties()
                    .await
                    .ok()
                    .flatten()
                    .map_or(0, |props| props.services.len());
                println!(
                    "    {:?} connected={:?} uuids={uuids}",
                    p.id(),
                    p.is_connected().await
                );
            }
        }
        Err(e) => println!("\n[C] retrieve_peripherals(全量) 失败: {e}"),
    }

    println!(
        "\n探针结束：以上为空即「本机没有可用的已连接 BLE 电池设备」；provider 侧会以 TransportDown 结构化上报，不 panic。"
    );
}

/// 后端不可用时的统一结论：不 panic，也不假装成功。
fn unavailable(reason: &str) {
    println!("\n[不可用] {reason}");
    println!(
        "结论：本机 BLE 后端不可用（provider 会据此上报 ProviderEvent::TransportDown，无 panic、无空读数）。"
    );
}

/// 对一台设备做一次真实读取：连接 → 发现服务 → 名称 → 电量。
async fn probe_device(p: &Peripheral) {
    println!("\n-- 读取 {:?}", p.id());
    match timeout(Duration::from_secs(4), p.connect()).await {
        Ok(Ok(())) => println!("   connect ok"),
        Ok(Err(e)) => {
            let connected = p.is_connected().await.unwrap_or(false);
            println!("   connect err: {e}（is_connected={connected}）");
            if !connected {
                return;
            }
        }
        Err(_) => {
            println!("   connect timeout");
            return;
        }
    }
    match timeout(Duration::from_secs(3), p.discover_services()).await {
        Ok(Ok(())) => println!("   discover ok"),
        Ok(Err(e)) => println!("   discover err: {e}"),
        Err(_) => println!("   discover timeout"),
    }
    let services: Vec<String> = p.services().iter().map(|s| s.uuid.to_string()).collect();
    println!("   services = {services:?}");
    let chars: Vec<String> = p
        .characteristics()
        .into_iter()
        .map(|c| format!("{} {:?}", c.uuid, c.properties))
        .collect();
    println!("   chars = {chars:?}");

    if let Some(ch) = p
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == uuid16(0x2A00))
    {
        match timeout(Duration::from_secs(2), p.read(&ch)).await {
            Ok(Ok(v)) => println!("   gap 0x2A00 = {:?}", String::from_utf8_lossy(&v)),
            Ok(Err(e)) => println!("   gap 0x2A00 read err: {e}"),
            Err(_) => println!("   gap 0x2A00 read timeout"),
        }
    } else {
        println!("   未找到 0x2A00");
    }

    if let Some(ch) = p
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == uuid16(0x2A19))
    {
        match timeout(Duration::from_secs(2), p.read(&ch)).await {
            Ok(Ok(v)) => println!("   battery 0x2A19 = {v:?}"),
            Ok(Err(e)) => println!("   battery read err: {e}"),
            Err(_) => println!("   battery read timeout"),
        }
    } else {
        println!("   未找到 0x2A19");
    }
}

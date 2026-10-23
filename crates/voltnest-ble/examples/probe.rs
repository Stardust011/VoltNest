//! 诊断探针：验证 Windows 上「已连接 BLE 设备」的两条枚举路径差异。
//!
//! 运行：`cargo run -p voltnest-ble --example probe`
//!
//! 结论用于确认 `Adapter::peripherals()`（只返回本次会话扫描到的广播设备）
//! 与 `Adapter::retrieve_peripherals()`（WinRT 已连接设备枚举）在本机的实际行为。

use btleplug::api::{Central, Manager as _, Peripheral as _, RetrievePeripheralsOptions};
use btleplug::platform::Manager;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

fn uuid16(short: u16) -> Uuid {
    Uuid::from_u128(u128::from(short) << 96 | 0x0000_1000_8000_0080_5f9b_34fb)
}

#[tokio::main]
async fn main() {
    let manager = Manager::new().await.expect("Manager 初始化失败");
    let adapters = manager.adapters().await.expect("枚举适配器失败");
    let adapter = adapters.into_iter().next().expect("没有 BLE 适配器");
    println!("adapter_info = {:?}", adapter.adapter_info().await);
    println!("adapter_state = {:?}", adapter.adapter_state().await);

    let plain = adapter.peripherals().await.expect("peripherals 失败");
    println!("\n[A] Adapter::peripherals() -> {} 台", plain.len());
    for p in &plain {
        println!("    {:?} connected={:?}", p.id(), p.is_connected().await);
    }

    let battery = uuid16(0x180F);
    let retrieved = adapter
        .retrieve_peripherals(RetrievePeripheralsOptions {
            identifiers: None,
            services: Some(vec![battery]),
        })
        .await
        .expect("retrieve_peripherals 失败");
    println!(
        "\n[B] retrieve_peripherals(services=[0x180F]) -> {} 台",
        retrieved.len()
    );
    for p in &retrieved {
        println!("    {:?} connected={:?}", p.id(), p.is_connected().await);
    }

    for p in retrieved {
        println!("\n-- 读取 {:?}", p.id());
        match timeout(Duration::from_secs(4), p.connect()).await {
            Ok(Ok(())) => println!("   connect ok"),
            Ok(Err(e)) => {
                println!("   connect err: {e}");
                continue;
            }
            Err(_) => {
                println!("   connect timeout");
                continue;
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
        if let Some(ch) = p.characteristics().into_iter().find(|c| c.uuid == uuid16(0x2A19)) {
            match timeout(Duration::from_secs(2), p.read(&ch)).await {
                Ok(Ok(v)) => println!("   battery 0x2A19 = {v:?}"),
                Ok(Err(e)) => println!("   battery read err: {e}"),
                Err(_) => println!("   battery read timeout"),
            }
        } else {
            println!("   未找到 0x2A19");
        }
    }
}

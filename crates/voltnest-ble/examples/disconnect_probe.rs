//! 断连事件探针：验证 `Adapter::events()` 的 `DeviceDisconnected` 何时到达。
//!
//! `cargo run -p voltnest-ble --example disconnect_probe -- self`：建链后 3s 主动 `disconnect()`；
//! `-- wait`：建链后只等，直到把外设电源关掉，验证真实的物理断开。

use btleplug::api::{Central, Manager as _, Peripheral as _, RetrievePeripheralsOptions};
use btleplug::platform::Manager;
use futures::StreamExt;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn uuid16(short: u16) -> Uuid {
    Uuid::from_u128(u128::from(short) << 96 | 0x0000_1000_8000_0080_5f9b_34fb)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "self".into());
    let manager = Manager::new().await?;
    let adapter = manager.adapters().await?.into_iter().next().unwrap();
    let start = Instant::now();

    let mut events = adapter.events().await?;

    let peripherals = adapter
        .retrieve_peripherals(RetrievePeripheralsOptions {
            identifiers: None,
            services: Some(vec![uuid16(0x180F)]),
        })
        .await?;
    println!(
        "[{}ms] 已连接且有 0x180F 的设备 {} 台",
        start.elapsed().as_millis(),
        peripherals.len()
    );
    let Some(p) = peripherals.into_iter().next() else {
        println!("[{}ms] 尝试扫描 6s ...", start.elapsed().as_millis());
        adapter.start_scan(Default::default()).await?;
        tokio::time::sleep(Duration::from_secs(6)).await;
        let found = adapter.peripherals().await?;
        println!(
            "[{}ms] 扫到 {} 台",
            start.elapsed().as_millis(),
            found.len()
        );
        let _ = adapter.stop_scan().await;
        for f in found.iter().take(10) {
            let name = f
                .properties()
                .await
                .ok()
                .flatten()
                .and_then(|p| p.local_name);
            println!("    {:?} name={:?}", f.address(), name);
        }
        return Ok(());
    };
    let addr = p.address();
    println!("[{}ms] 选中 {:?}", start.elapsed().as_millis(), addr);

    println!("[{}ms] connect...", start.elapsed().as_millis());
    p.connect().await?;
    p.discover_services().await?;
    println!(
        "[{}ms] 已连接: is_connected={:?} 服务 {} 个",
        start.elapsed().as_millis(),
        p.is_connected().await,
        p.services().len()
    );

    if mode == "self" {
        tokio::time::sleep(Duration::from_secs(3)).await;
        println!("[{}ms] 主动 disconnect()", start.elapsed().as_millis());
        p.disconnect().await?;
    } else {
        println!("现在请关闭外设电源 / 移出范围，等待 DeviceDisconnected ...");
    }

    let deadline = tokio::time::sleep(Duration::from_secs(120));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            item = events.next() => match item {
                Some(ev) => {
                    println!("[{}ms] event: {:?}", start.elapsed().as_millis(), ev);
                    if matches!(ev, btleplug::api::CentralEvent::DeviceDisconnected(_)) {
                        println!(
                            "[{}ms] 断连事件到达，距断开动作 {}ms",
                            start.elapsed().as_millis(),
                            start.elapsed().as_millis().saturating_sub(3_000)
                        );
                        break;
                    }
                }
                None => {
                    println!("事件流结束");
                    break;
                }
            },
            () = &mut deadline => {
                println!("[{}ms] 120s 内没有断连事件", start.elapsed().as_millis());
                break;
            }
        }
    }
    println!(
        "[{}ms] is_connected={:?}",
        start.elapsed().as_millis(),
        p.is_connected().await
    );
    Ok(())
}

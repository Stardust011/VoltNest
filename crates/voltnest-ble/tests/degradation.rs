//! 降级契约：**后端缺席（无适配器 / bluetoothd 未运行 / 权限不足）时不得 panic，也不得空转挂住**。
//!
//! 真机采集无法在 CI 上验证，但「没有硬件时怎么收场」可以：
//! 采集线程要么上报一次 ProviderEvent::TransportDown 后正常结束，要么在有后端时继续运行，
//! 两种情形都必须能被停止标志结束并正常返回。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use voltnest_ble::provider::BleProvider;
use voltnest_provider::{Provider, ProviderEvent};

#[test]
fn missing_backend_degrades_without_panic_or_hang() {
    let (event_tx, event_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    std::thread::spawn(move || {
        Box::new(BleProvider::new()).run(event_tx, stop_thread);
        // run 返回 = 采集线程正常收场（panic 会在测试进程里报线程崩溃）。
        let _ = done_tx.send(());
    });

    // 无后端的机器上这里必定先收到 TransportDown；有后端时不发事件，走下面的停止分支。
    if let Ok(event) = event_rx.recv_timeout(Duration::from_secs(3))
        && let ProviderEvent::TransportDown { code, detail } = &event
    {
        assert!(!detail.is_empty(), "TransportDown 必须带可读 detail");
        println!("降级路径命中：{code} — {detail}");
    }

    stop.store(true, Ordering::Relaxed);
    // 5s 足够覆盖 250ms 的巡检 tick；拿不到就说明线程卡住了（这才是要防的「阻塞」）。
    assert!(
        done_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
        "采集线程未在停止标志后退出"
    );
}

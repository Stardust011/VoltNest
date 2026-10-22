//! BLE 调度策略：负缓存、退避、跟踪上限、并发上限（O-1..O-8）。
//!
//! 本模块是**纯逻辑**：时间由调用方以单调毫秒传入（Provider 内部巡检用
//! `Instant`，不参与任何对外语义，D10）。这样全部边界都能用「手动推进的
//! 假时钟」测出来，不需要注入时钟抽象。

use crate::{
    BACKOFF_CAP_MS, CONNECT_TIMEOUT_MS, DISCOVER_TIMEOUT_MS, FAILURES_BEFORE_BACKOFF,
    MAX_CONCURRENT, MAX_TRACKED, NEGATIVE_CACHE_MS, READ_TIMEOUT_MS,
};
use std::collections::BTreeMap;
use voltnest_provider::ErrorCode;

/// BLE 调度参数。默认值全部来自规格 §6 与实测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlePolicy {
    /// 建链超时。
    pub connect_timeout_ms: u64,
    /// 服务发现超时。
    pub discover_timeout_ms: u64,
    /// 读取超时。
    pub read_timeout_ms: u64,
    /// 无 `0x180F` 的负缓存 TTL。
    pub negative_cache_ms: u64,
    /// 跟踪上限。
    pub max_tracked: usize,
    /// 全局同时建链上限。
    pub max_concurrent: usize,
    /// 连续失败多少次后开始退避。
    pub failures_before_backoff: u32,
    /// 退避上限。
    pub backoff_cap_ms: u64,
}

impl Default for BlePolicy {
    fn default() -> Self {
        Self {
            connect_timeout_ms: CONNECT_TIMEOUT_MS,
            discover_timeout_ms: DISCOVER_TIMEOUT_MS,
            read_timeout_ms: READ_TIMEOUT_MS,
            negative_cache_ms: NEGATIVE_CACHE_MS,
            max_tracked: MAX_TRACKED,
            max_concurrent: MAX_CONCURRENT,
            failures_before_backoff: FAILURES_BEFORE_BACKOFF,
            backoff_cap_ms: BACKOFF_CAP_MS,
        }
    }
}

/// 本轮是否可以对该地址建链。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Admission {
    /// 可以建链。
    Admit,
    /// 负缓存命中：该地址没有 Battery Service，`until_ms` 之前不再摸它。
    NegativeCached {
        /// 缓存到期时间（单调毫秒）。
        until_ms: u64,
    },
    /// 退避中。
    BackingOff {
        /// 退避结束时间（单调毫秒）。
        until_ms: u64,
    },
    /// 超过跟踪上限（O-6）：只进诊断页，不建链。
    OverLimit,
}

/// 单个地址的策略状态。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DevicePolicy {
    /// 连续失败次数。
    pub failures: u32,
    /// 退避结束时间。
    pub backoff_until_ms: Option<u64>,
    /// 负缓存到期时间。
    pub negative_until_ms: Option<u64>,
    /// 最近一次错误码（诊断）。
    pub last_error: Option<ErrorCode>,
}

/// 全部地址的策略簿。只有协调线程持有它（I/O 执行器无状态）。
#[derive(Clone, Debug)]
pub struct PolicyBook {
    policy: BlePolicy,
    devices: BTreeMap<[u8; 6], DevicePolicy>,
}

impl PolicyBook {
    /// 构造。
    pub fn new(policy: BlePolicy) -> Self {
        Self {
            policy,
            devices: BTreeMap::new(),
        }
    }

    /// 当前参数。
    pub fn policy(&self) -> &BlePolicy {
        &self.policy
    }

    /// 是否还有并发槽位（O-4）。
    pub fn allows_concurrent(&self, running: usize) -> bool {
        running < self.policy.max_concurrent
    }

    /// 已跟踪的地址数。
    pub fn tracked(&self) -> usize {
        self.devices.len()
    }

    /// 登记一个地址（尚未建链）。
    pub fn track(&mut self, addr: [u8; 6]) {
        self.devices.entry(addr).or_default();
    }

    /// 判定本轮是否建链。
    pub fn admission(&self, addr: [u8; 6], now_ms: u64) -> Admission {
        let Some(state) = self.devices.get(&addr) else {
            // 未登记的新地址：只有还有槽位时才允许。
            return if self.devices.len() < self.policy.max_tracked {
                Admission::Admit
            } else {
                Admission::OverLimit
            };
        };
        if let Some(until) = state.negative_until_ms
            && now_ms < until
        {
            return Admission::NegativeCached { until_ms: until };
        }
        if let Some(until) = state.backoff_until_ms
            && now_ms < until
        {
            return Admission::BackingOff { until_ms: until };
        }
        Admission::Admit
    }

    /// 建链 + 读取成功：清零失败计数、退避与负缓存（O-5「重新 connected 后清零」）。
    pub fn on_success(&mut self, addr: [u8; 6]) {
        let state = self.devices.entry(addr).or_default();
        state.failures = 0;
        state.backoff_until_ms = None;
        state.negative_until_ms = None;
        state.last_error = None;
    }

    /// 一次普通失败（超时、读失败、会话报错）。
    pub fn on_failure(&mut self, addr: [u8; 6], now_ms: u64, code: ErrorCode) {
        let threshold = self.policy.failures_before_backoff;
        let cap = self.policy.backoff_cap_ms;
        let state = self.devices.entry(addr).or_default();
        state.failures = state.failures.saturating_add(1);
        state.last_error = Some(code);
        if state.failures >= threshold {
            let step = backoff_step(state.failures, threshold, cap);
            state.backoff_until_ms = Some(now_ms.saturating_add(step));
        }
    }

    /// 服务缺失：写负缓存并把失败计数清零（这不是「设备坏了」，只是「不是电量设备」）。
    pub fn on_service_missing(&mut self, addr: [u8; 6], now_ms: u64) {
        let ttl = self.policy.negative_cache_ms;
        let state = self.devices.entry(addr).or_default();
        state.failures = 0;
        state.backoff_until_ms = None;
        state.negative_until_ms = Some(now_ms.saturating_add(ttl));
        state.last_error = Some(ErrorCode::ServiceMissing);
    }

    /// 设备从系统消失：丢弃策略状态（下次出现按新设备处理）。
    pub fn forget(&mut self, addr: [u8; 6]) {
        self.devices.remove(&addr);
    }

    /// 只读查询（诊断页）。
    pub fn state(&self, addr: [u8; 6]) -> Option<&DevicePolicy> {
        self.devices.get(&addr)
    }

    /// 负缓存中的地址（诊断页「已连接但无电量服务」一节）。
    pub fn negative_cached(&self, now_ms: u64) -> Vec<[u8; 6]> {
        self.devices
            .iter()
            .filter(|(_, s)| s.negative_until_ms.is_some_and(|until| now_ms < until))
            .map(|(addr, _)| *addr)
            .collect()
    }
}

/// 退避步长：第 3 次失败 2 分钟，之后 4 / 8 分钟，上限 10 分钟（O-5）。
pub fn backoff_step(failures: u32, threshold: u32, cap_ms: u64) -> u64 {
    if failures < threshold {
        return 0;
    }
    let exp = failures - threshold;
    // 2 分钟起步，指数增长，避免 2u64.pow 溢出（最多只用到 3~4 档）。
    let factor = 1u64.checked_shl(exp.min(20)).unwrap_or(u64::MAX);
    (2 * 60_000u64).saturating_mul(factor).min(cap_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: [u8; 6] = [0x01, 0x00, 0x00, 0x23, 0x04, 0x35];
    const T: u64 = 1_000_000;

    fn book() -> PolicyBook {
        PolicyBook::new(BlePolicy::default())
    }

    #[test]
    fn defaults_match_spec() {
        let p = BlePolicy::default();
        assert_eq!(p.connect_timeout_ms, 4_000);
        assert_eq!(p.discover_timeout_ms, 3_000);
        assert_eq!(p.read_timeout_ms, 2_000);
        assert_eq!(p.negative_cache_ms, 30 * 60_000);
        assert_eq!(p.max_tracked, 8);
        assert_eq!(p.max_concurrent, 2);
    }

    #[test]
    fn new_device_is_admitted() {
        assert_eq!(book().admission(A, T), Admission::Admit);
    }

    #[test]
    fn negative_cache_blocks_for_thirty_minutes() {
        // O-2：否则用户每台非电量 BLE 外设会每 60s 被建链一次，永不停止。
        let mut b = book();
        b.track(A);
        b.on_service_missing(A, T);
        assert_eq!(
            b.admission(A, T + 30 * 60_000 - 1),
            Admission::NegativeCached {
                until_ms: T + 30 * 60_000
            }
        );
        assert_eq!(b.admission(A, T + 30 * 60_000), Admission::Admit);
    }

    #[test]
    fn negative_cache_survives_failures() {
        let mut b = book();
        b.on_service_missing(A, T);
        b.on_failure(A, T + 1, ErrorCode::IoTimeout);
        assert!(matches!(
            b.admission(A, T + 2),
            Admission::NegativeCached { .. }
        ));
    }

    #[test]
    fn backoff_is_two_four_eight_then_cap() {
        // O-5：连续失败 3 次 → 2 / 4 / 8 分钟，上限 10 分钟。
        let mut b = book();
        b.track(A);
        b.on_failure(A, T, ErrorCode::IoTimeout);
        b.on_failure(A, T, ErrorCode::IoTimeout);
        assert_eq!(b.admission(A, T), Admission::Admit, "前两次不进入退避");
        b.on_failure(A, T, ErrorCode::IoTimeout);
        assert_eq!(
            b.admission(A, T),
            Admission::BackingOff {
                until_ms: T + 120_000
            }
        );

        let mut b = book();
        for _ in 0..4 {
            b.on_failure(A, T, ErrorCode::IoTimeout);
        }
        assert_eq!(
            b.admission(A, T),
            Admission::BackingOff {
                until_ms: T + 240_000
            }
        );

        let mut b = book();
        for _ in 0..5 {
            b.on_failure(A, T, ErrorCode::IoTimeout);
        }
        assert_eq!(
            b.admission(A, T),
            Admission::BackingOff {
                until_ms: T + 480_000
            }
        );

        let mut b = book();
        for _ in 0..9 {
            b.on_failure(A, T, ErrorCode::IoTimeout);
        }
        assert_eq!(
            b.admission(A, T),
            Admission::BackingOff {
                until_ms: T + 600_000
            }
        );
    }

    #[test]
    fn backoff_expires() {
        let mut b = book();
        for _ in 0..3 {
            b.on_failure(A, T, ErrorCode::IoTimeout);
        }
        assert!(matches!(
            b.admission(A, T + 1),
            Admission::BackingOff { .. }
        ));
        assert_eq!(b.admission(A, T + 120_000), Admission::Admit);
    }

    #[test]
    fn success_resets_everything() {
        let mut b = book();
        for _ in 0..5 {
            b.on_failure(A, T, ErrorCode::IoTimeout);
        }
        b.on_success(A);
        assert_eq!(b.admission(A, T), Admission::Admit);
        let state = b.state(A).unwrap();
        assert_eq!(state.failures, 0);
        assert!(state.backoff_until_ms.is_none());
        assert!(state.last_error.is_none());
    }

    #[test]
    fn service_missing_resets_failures() {
        let mut b = book();
        b.on_failure(A, T, ErrorCode::IoTimeout);
        b.on_service_missing(A, T);
        assert_eq!(b.state(A).unwrap().failures, 0);
        assert_eq!(
            b.state(A).unwrap().last_error,
            Some(ErrorCode::ServiceMissing)
        );
    }

    #[test]
    fn tracking_limit_is_eight() {
        // O-6：第 9 台只进诊断页，不建链。
        let mut b = book();
        for i in 0..8u8 {
            b.track([i, 0, 0, 0, 0, 1]);
        }
        assert_eq!(b.tracked(), 8);
        assert_eq!(b.admission([8, 0, 0, 0, 0, 1], T), Admission::OverLimit);
    }

    #[test]
    fn concurrency_limit_is_two() {
        let b = book();
        assert!(b.allows_concurrent(0));
        assert!(b.allows_concurrent(1));
        assert!(!b.allows_concurrent(2), "O-4：全局同时建链 ≤ 2");
    }

    #[test]
    fn forget_removes_state() {
        let mut b = book();
        b.track(A);
        b.forget(A);
        assert_eq!(b.tracked(), 0);
        assert!(b.state(A).is_none());
    }

    #[test]
    fn negative_cached_list_is_reported() {
        let mut b = book();
        b.on_service_missing(A, T);
        assert_eq!(b.negative_cached(T + 1), vec![A]);
        assert!(b.negative_cached(T + NEGATIVE_CACHE_MS).is_empty());
    }
}

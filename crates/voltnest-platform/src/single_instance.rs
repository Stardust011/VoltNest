//! 单实例：二次启动唤醒首实例，不新建窗口。
//!
//! Windows 用当前用户 Local 命名空间的互斥体 + 命名事件：第二个进程给命名事件发信号后立即退出，
//! 首实例里等待该事件的线程把已有窗口 show + unminimize + 抢焦点。
//! 用 Local 而非 Global：多用户同时登录时各自应有一个实例。
//!
//! Linux 用 Unix domain socket：首实例 bind + listen（$XDG_RUNTIME_DIR/voltnest.sock，
//! 缺省 /tmp/voltnest-<uid>.sock），第二个进程 connect 成功后直接退出，首实例的监听线程
//! 收到连接即触发 activation 回调。
//!
//! 两平台一致：拿不到锁一律降级为 Unavailable 而不是阻塞启动——宁可多开一个窗口，也不要打不开。

#[cfg(windows)]
mod imp {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject,
    };

    /// 互斥体名（当前用户 Local 命名空间）。
    pub const MUTEX_NAME: &str = "Local\\VoltNest.SingleInstance.Mutex";
    /// 唤醒事件名。
    pub const EVENT_NAME: &str = "Local\\VoltNest.SingleInstance.Activate";

    const EVENT_MODIFY_STATE: u32 = 0x0002;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// windows-sys 0.61 起 HANDLE 是裸指针，默认不 Send；内核句柄实际可跨线程用于
    /// SetEvent / WaitForSingleObject / CloseHandle，故在此显式承担该保证，仅限本模块使用。
    #[derive(Clone, Copy)]
    struct Handle(HANDLE);

    // SAFETY: 句柄只是内核对象引用，Windows 允许跨线程使用；关闭的唯一责任方是 Guard::drop。
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}

    impl Handle {
        fn is_null(self) -> bool {
            self.0.is_null()
        }
    }

    /// 单实例判定结果。
    pub enum Instance {
        /// 本进程是第一个实例，持有互斥体与事件。
        First(Guard),
        /// 已有实例在运行，本进程应发信号后退出。
        Second,
        /// 创建失败（权限异常等）：不阻塞启动，按首实例继续跑。
        Unavailable(String),
    }

    /// 首实例持有的内核对象；Drop 时关闭，进程退出后互斥体自动释放。
    pub struct Guard {
        mutex: Handle,
        event: Handle,
    }

    impl Guard {
        /// 通知等待中的首实例唤醒窗口。
        pub fn activate_existing(&self) -> bool {
            if self.event.is_null() {
                return false;
            }
            // SAFETY: event 由本结构独占持有，调用期间不会关闭。
            unsafe { SetEvent(self.event.0) != 0 }
        }

        /// 起线程等待「二次启动」信号；stop 置位后线程自行退出。
        pub fn listen_activation(
            &self,
            stop: Arc<AtomicBool>,
            on_activate: impl Fn() + Send + 'static,
        ) -> JoinHandle<()> {
            let event = self.event;
            std::thread::Builder::new()
                .name("voltnest-activation".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        if event.is_null() {
                            return;
                        }
                        // SAFETY: event 在 Guard 存活期间有效；退出流程先置 stop，最坏多循环一次。
                        let rc = unsafe { WaitForSingleObject(event.0, 500) };
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        if rc == WAIT_OBJECT_0 {
                            on_activate();
                        }
                    }
                })
                .expect("激活监听线程创建失败")
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: 句柄由本结构独占，且只关闭一次。
            unsafe {
                if !self.mutex.is_null() {
                    CloseHandle(self.mutex.0);
                }
                if !self.event.is_null() {
                    CloseHandle(self.event.0);
                }
            }
        }
    }

    /// 获取单实例判定。
    pub fn acquire() -> Instance {
        let mutex_name = wide(MUTEX_NAME);
        // SAFETY: 名称以 NUL 结尾的 UTF-16；句柄由 Guard 关闭。
        let mutex = Handle(unsafe { CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr()) });
        if mutex.is_null() {
            return Instance::Unavailable(format!(
                "CreateMutexW 失败: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: 紧随 CreateMutexW 读取，语义正确。
        let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if already {
            // SAFETY: 本进程拿到的句柄仍需关闭，否则泄漏。
            unsafe { CloseHandle(mutex.0) };
            signal_existing();
            return Instance::Second;
        }

        let event_name = wide(EVENT_NAME);
        // SAFETY: 名称以 NUL 结尾 UTF-16；句柄由 Guard 关闭。
        let event = Handle(unsafe { CreateEventW(std::ptr::null(), 1, 0, event_name.as_ptr()) });
        Instance::First(Guard { mutex, event })
    }

    /// 第二实例：打开事件并置位，由调用方退出进程。
    fn signal_existing() {
        let event_name = wide(EVENT_NAME);
        // SAFETY: 打开已有事件句柄，失败返回空指针。
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr()) };
        if !event.is_null() {
            // SAFETY: 句柄由本函数打开并关闭。
            unsafe {
                SetEvent(event);
                CloseHandle(event);
            }
        }
    }
}

#[cfg(windows)]
pub use imp::{EVENT_NAME, Guard, Instance, MUTEX_NAME, acquire};

/// Linux：Unix domain socket。首实例 bind + listen，第二次启动 connect 一下就算唤醒。
#[cfg(target_os = "linux")]
mod imp_linux {
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use std::time::Duration;

    /// socket 文件名（放在 XDG_RUNTIME_DIR 下）。
    pub const SOCKET_FILE_NAME: &str = "voltnest.sock";
    /// XDG_RUNTIME_DIR 不可用时的回退目录。
    pub const FALLBACK_SOCKET_DIR: &str = "/tmp";
    /// 监听线程的轮询间隔：非阻塞 accept + 小睡，既能及时响应，又不会空转烧 CPU。
    const POLL_INTERVAL: Duration = Duration::from_millis(50);

    /// 单实例判定结果。
    pub enum Instance {
        /// 本进程是第一个实例，持有监听 socket。
        First(Guard),
        /// 已有实例在运行，本进程已唤醒它，应退出。
        Second,
        /// 无法建立 socket（权限异常等）：不阻塞启动，按首实例继续跑。
        Unavailable(String),
    }

    /// 首实例持有的监听 socket；Drop 时删除 socket 文件。
    pub struct Guard {
        path: PathBuf,
        listener: UnixListener,
    }

    impl Guard {
        /// 通知首实例唤醒窗口（Linux 即连接自己持有的监听 socket）。
        pub fn activate_existing(&self) -> bool {
            notify(&self.path)
        }

        /// 起线程等待「二次启动」连接；stop 置位后线程自行退出。
        pub fn listen_activation(
            &self,
            stop: Arc<AtomicBool>,
            on_activate: impl Fn() + Send + 'static,
        ) -> JoinHandle<()> {
            let Ok(listener) = self.listener.try_clone() else {
                return std::thread::spawn(|| {});
            };
            if listener.set_nonblocking(true).is_err() {
                return std::thread::spawn(|| {});
            }
            std::thread::Builder::new()
                .name("voltnest-activation".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((_stream, _addr)) => {
                                if !stop.load(Ordering::Relaxed) {
                                    on_activate();
                                }
                            }
                            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(POLL_INTERVAL);
                            }
                            Err(_) => std::thread::sleep(POLL_INTERVAL),
                        }
                    }
                })
                .expect("激活监听线程创建失败")
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// 由 $XDG_RUNTIME_DIR 派生 socket 路径；相对路径或未设置时回退到 /tmp（纯逻辑，便于测试）。
    pub fn socket_path_from(runtime_dir: Option<&Path>, uid: Option<u32>) -> PathBuf {
        match runtime_dir {
            Some(dir) if dir.is_absolute() => dir.join(SOCKET_FILE_NAME),
            _ => {
                let name = match uid {
                    Some(uid) => format!("voltnest-{uid}.sock"),
                    None => "voltnest-anon.sock".to_string(),
                };
                Path::new(FALLBACK_SOCKET_DIR).join(name)
            }
        }
    }

    /// 当前用户的 socket 路径。
    pub fn socket_path() -> PathBuf {
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute() && dir.is_dir());
        socket_path_from(runtime_dir.as_deref(), current_uid())
    }

    /// 当前用户 uid；Linux 上 /proc/self 的属主就是本进程属主。
    fn current_uid() -> Option<u32> {
        std::fs::metadata("/proc/self").ok().map(|meta| meta.uid())
    }

    /// 文件属主不是本用户（只在能确定 uid 时才下结论）。
    fn foreign_owner(meta: &std::fs::Metadata) -> bool {
        current_uid().is_some_and(|uid| meta.uid() != uid)
    }

    /// 连接首实例并写一个字节；连上即说明有人 listen。
    fn notify(path: &Path) -> bool {
        match UnixStream::connect(path) {
            Ok(mut stream) => {
                let _ = stream.write_all(b"\n");
                true
            }
            Err(_) => false,
        }
    }

    /// 获取单实例判定。
    pub fn acquire() -> Instance {
        acquire_with(&socket_path())
    }

    /// acquire 的可注入版本：路径由调用方给出，便于在临时目录里测真实 socket。
    fn acquire_with(path: &Path) -> Instance {
        if notify(path) {
            return Instance::Second;
        }
        match std::fs::metadata(path) {
            Ok(meta) => {
                if foreign_owner(&meta) {
                    return Instance::Unavailable(format!(
                        "{} 属于其它用户，放弃单实例",
                        path.display()
                    ));
                }
                if let Err(err) = std::fs::remove_file(path)
                    && err.kind() != std::io::ErrorKind::NotFound
                {
                    return Instance::Unavailable(format!("清理残留 socket 失败：{err}"));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Instance::Unavailable(format!("检查 socket 路径失败：{err}")),
        }
        match bind(path) {
            Ok(listener) => Instance::First(Guard {
                path: path.to_path_buf(),
                listener,
            }),
            Err(err) => {
                if notify(path) {
                    return Instance::Second;
                }
                Instance::Unavailable(format!("绑定 {} 失败：{err}", path.display()))
            }
        }
    }

    /// bind + listen，并把文件权限收紧到属主可读写。
    ///
    /// /tmp 回退路径上没有 0700 目录兜底，收紧权限让别的用户连不进来（只能造成拒绝服务，
    /// 泄漏不了数据，但能收紧就收紧）。
    fn bind(path: &Path) -> std::io::Result<UnixListener> {
        let listener = UnixListener::bind(path)?;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        Ok(listener)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 每次用独立文件名，避免测试并行时互相踩 socket。
        fn temp_socket(name: &str) -> PathBuf {
            let path = std::env::temp_dir()
                .join(format!("voltnest-test-{}-{name}.sock", std::process::id()));
            let _ = std::fs::remove_file(&path);
            path
        }

        fn first_or_panic(path: &Path) -> Guard {
            match acquire_with(path) {
                Instance::First(guard) => guard,
                Instance::Second => panic!("首次获取不应是 Second"),
                Instance::Unavailable(reason) => panic!("首次获取失败：{reason}"),
            }
        }

        #[test]
        fn runtime_dir_wins_when_absolute() {
            assert_eq!(
                socket_path_from(Some(Path::new("/run/user/1000")), Some(1000)),
                PathBuf::from("/run/user/1000/voltnest.sock")
            );
        }

        #[test]
        fn fallback_carries_uid() {
            assert_eq!(
                socket_path_from(None, Some(1000)),
                PathBuf::from("/tmp/voltnest-1000.sock")
            );
        }

        #[test]
        fn relative_runtime_dir_is_ignored() {
            assert_eq!(
                socket_path_from(Some(Path::new("run/user/1000")), Some(7)),
                PathBuf::from("/tmp/voltnest-7.sock")
            );
        }

        #[test]
        fn unknown_uid_falls_back_to_anon_name() {
            assert_eq!(
                socket_path_from(None, None),
                PathBuf::from("/tmp/voltnest-anon.sock")
            );
        }

        #[test]
        fn first_then_second_instance() {
            let path = temp_socket("first-second");
            let guard = first_or_panic(&path);
            assert!(
                matches!(acquire_with(&path), Instance::Second),
                "首实例存活时第二次获取应为 Second"
            );
            drop(guard);
            assert!(!path.exists(), "Guard 释放后应清掉 socket 文件");
        }

        #[test]
        fn stale_socket_file_self_heals() {
            let path = temp_socket("stale");
            let _ = UnixListener::bind(&path).expect("绑定测试 socket 失败");
            assert!(path.exists(), "残留文件应仍在");

            let guard = match acquire_with(&path) {
                Instance::First(guard) => guard,
                Instance::Second => panic!("残留文件不应被判为 Second"),
                Instance::Unavailable(reason) => panic!("残留文件应能自愈，实际：{reason}"),
            };
            assert!(matches!(acquire_with(&path), Instance::Second));
            drop(guard);
        }

        #[test]
        fn bind_failure_degrades_to_unavailable() {
            let path = std::env::temp_dir()
                .join("voltnest-test-missing-dir")
                .join("nested.sock");
            match acquire_with(&path) {
                Instance::Unavailable(reason) => assert!(reason.contains("绑定"), "{reason}"),
                Instance::First(guard) => {
                    drop(guard);
                    panic!("父目录不存在时不应 bind 成功");
                }
                Instance::Second => panic!("不应判为 Second"),
            }
        }

        #[test]
        fn second_instance_wakes_first() {
            let path = temp_socket("wake");
            let guard = first_or_panic(&path);

            let (tx, rx) = std::sync::mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let _listener = guard.listen_activation(Arc::clone(&stop), move || {
                let _ = tx.send(());
            });

            assert!(
                matches!(acquire_with(&path), Instance::Second),
                "第二实例应成功"
            );
            rx.recv_timeout(Duration::from_secs(10))
                .expect("首实例应被第二次启动唤醒");

            stop.store(true, Ordering::Relaxed);
            drop(guard);
        }
    }
}

#[cfg(target_os = "linux")]
pub use imp_linux::{
    FALLBACK_SOCKET_DIR, Guard, Instance, SOCKET_FILE_NAME, acquire, socket_path, socket_path_from,
};

/// 其它平台：没有锁实现，恒为首实例（不阻塞启动）。
#[cfg(all(not(windows), not(target_os = "linux")))]
mod imp_other {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::thread::JoinHandle;

    /// 单实例判定结果。
    pub enum Instance {
        /// 首实例。
        First(Guard),
        /// 已有实例。
        Second,
        /// 不可用。
        Unavailable(String),
    }

    /// 空守卫。
    pub struct Guard;

    impl Guard {
        /// 空操作。
        pub fn activate_existing(&self) -> bool {
            false
        }

        /// 空监听。
        pub fn listen_activation(
            &self,
            _stop: Arc<AtomicBool>,
            _on_activate: impl Fn() + Send + 'static,
        ) -> JoinHandle<()> {
            std::thread::spawn(|| {})
        }
    }

    /// 恒为首实例。
    pub fn acquire() -> Instance {
        Instance::First(Guard)
    }
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub use imp_other::{Guard, Instance, acquire};

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_reports_second_instance() {
        let first = match acquire() {
            Instance::First(g) => g,
            Instance::Second => panic!("首次获取不应是 Second"),
            Instance::Unavailable(e) => panic!("首次获取失败: {e}"),
        };
        assert!(matches!(acquire(), Instance::Second));
        assert!(first.activate_existing(), "置位命名事件应成功");
        drop(first);
    }

    #[test]
    fn names_are_user_local() {
        assert!(MUTEX_NAME.starts_with("Local\\"));
        assert!(EVENT_NAME.starts_with("Local\\"));
    }
}

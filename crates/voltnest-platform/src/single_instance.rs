//! 单实例：当前用户 `Local` 命名空间的互斥体 + 命名事件（AC-9 / AC-10）。
//!
//! 二次启动时**不新建窗口**：第二个进程给命名事件发信号后立即退出，
//! 第一个进程里等待该事件的线程把已有窗口 `show` + `unminimize` + 抢焦点。
//!
//! 用 `Local\` 前缀而不是 `Global\`：多用户同时登录时各自应有一个实例，
//! `Global\` 会让第二个用户完全无法启动应用。

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

    /// `windows-sys` 0.61 起 `HANDLE` 是裸指针，裸指针默认不 `Send`。
    /// 内核句柄本身可以跨线程用于 `SetEvent` / `WaitForSingleObject` / `CloseHandle`，
    /// 因此在这里显式承担该保证，并且只在本模块内使用。
    #[derive(Clone, Copy)]
    struct Handle(HANDLE);

    // SAFETY: 句柄值只是一个内核对象引用，Windows 允许跨线程使用；
    // 关闭的唯一责任方是 Guard::drop（单线程执行）。
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
        /// 创建失败（权限异常等）：不阻塞启动，只当作首个实例继续跑。
        Unavailable(String),
    }

    /// 首实例持有的内核对象；`Drop` 时关闭，进程退出后互斥体自动释放。
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

        /// 起一个线程等待「二次启动」信号。`stop` 置位后线程自行退出。
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
                            // 没有事件对象时无事可等，直接退出避免空转。
                            return;
                        }
                        // SAFETY: event 在 Guard 存活期间有效；调用方在退出流程里
                        // 先置 stop，等待窗口 500ms，最坏多循环一次。
                        let rc = unsafe { WaitForSingleObject(event.0, 500) };
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        if rc == WAIT_OBJECT_0 {
                            on_activate();
                        }
                    }
                })
                .expect("创建激活监听线程失败")
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
        // SAFETY: 名称是以 NUL 结尾的 UTF-16；返回句柄由 Guard 负责关闭。
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
            // SAFETY: 本进程拿到的句柄仍要关闭，否则句柄泄漏。
            unsafe { CloseHandle(mutex.0) };
            signal_existing();
            return Instance::Second;
        }

        let event_name = wide(EVENT_NAME);
        // 手动复位事件：首个实例创建，二次启动只 SetEvent。
        // SAFETY: 名称为 NUL 结尾 UTF-16；句柄由 Guard 关闭。
        let event = Handle(unsafe { CreateEventW(std::ptr::null(), 1, 0, event_name.as_ptr()) });
        // 事件创建失败不影响单实例语义，只是二次启动无法唤醒窗口。
        Instance::First(Guard { mutex, event })
    }

    /// 第二实例：打开事件并置位，然后由调用方退出进程。
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

#[cfg(not(windows))]
mod imp_non_windows {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::thread::JoinHandle;

    /// 单实例判定结果（非 Windows 只有「首实例」一种）。
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

        /// 空监听（非 Windows 不实现唤醒）。
        pub fn listen_activation(
            &self,
            _stop: Arc<AtomicBool>,
            _on_activate: impl Fn() + Send + 'static,
        ) -> JoinHandle<()> {
            std::thread::spawn(|| {})
        }
    }

    /// 非 Windows 恒为首实例。
    pub fn acquire() -> Instance {
        Instance::First(Guard)
    }
}

#[cfg(not(windows))]
pub use imp_non_windows::{Guard, Instance, acquire};

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
        // 同进程再次获取：互斥体已存在 → Second。
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

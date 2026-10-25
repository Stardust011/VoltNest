//! 开机自启：Windows 写 HKCU 的 Run 项，Linux 写 XDG Autostart 条目。
//!
//! Windows：HKCU\Software\Microsoft\Windows\CurrentVersion\Run 下的 VoltNest 值，由系统在登录后拉起。
//! Linux：$XDG_CONFIG_HOME/autostart/voltnest.desktop（缺省 ~/.config/autostart/），
//! 由桌面环境的 XDG Autostart 实现按条目里的 Exec 拉起进程。
//!
//! 永不默认开启，仅用户在引导页或设置页显式勾选才写入；卸载须清除；改动立即生效。
//! 命令恒带 --autostart，但首屏显隐与手动启动一致，统一由「启动时最小化到托盘」决定。

use std::path::{Path, PathBuf};

/// 注册表位置（Windows）。
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// 值名（Windows）。
pub const VALUE_NAME: &str = "VoltNest";

/// XDG Autostart 目录名（相对 XDG 配置根）。
pub const AUTOSTART_DIR: &str = "autostart";
/// 自启条目文件名。
pub const DESKTOP_FILE_NAME: &str = "voltnest.desktop";
/// 条目里的应用名；freedesktop 通知服务也用它当通知的应用名。
pub const DISPLAY_NAME: &str = "VoltNest";

/// 生成注册表命令 `"路径" --autostart`。
///
/// 路径可能含空格（用户可改安装目录）故按需加引号；结尾多余反斜杠会转义引号，先削掉。
pub fn run_command(exe: &Path) -> String {
    let mut path = exe.display().to_string();
    if !path.contains(' ') && !path.contains('\t') {
        return format!("{path} {}", crate::AUTOSTART_FLAG);
    }
    while path.ends_with('\\') && !path.ends_with("\\\\") {
        path.pop();
    }
    format!("\"{path}\" {}", crate::AUTOSTART_FLAG)
}

/// Exec 参数里必须加双引号的保留字符（Desktop Entry 规范 3.3 节）。
const EXEC_RESERVED: &str = " \t\n\"'\\><~|&;$*?#()`";

/// 单个 Exec 参数按 Desktop Entry 规范转义。
///
/// 含保留字符时整体加双引号，引号内的 " ` $ \ 再各自转义；% 一律写成 %%（否则会被当成域代码，
/// 例如 %f）；字面反斜杠写四个——值级转义先把一个反斜杠变成两个，Exec 级再各转义一次。
fn escape_exec_arg(arg: &str) -> String {
    let plain = !arg.is_empty() && !arg.chars().any(|c| EXEC_RESERVED.contains(c));
    if plain {
        return arg.replace('%', "%%");
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for c in arg.chars() {
        match c {
            '%' => out.push_str("%%"),
            '\\' => out.push_str("\\\\\\\\"),
            '"' => out.push_str("\\\""),
            c if c == '`' || c == '$' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 生成 .desktop 条目内容（纯函数，便于测试）。
///
/// Exec 指向当前可执行文件并恒带 --autostart；X-GNOME-Autostart-enabled=true 让 GNOME 系桌面
/// 不会因为用户此前在「启动应用程序」里手动禁用过而忽略本条目。
pub fn desktop_entry(exe: &Path) -> String {
    let exec = format!(
        "{} {}",
        escape_exec_arg(&exe.to_string_lossy()),
        crate::AUTOSTART_FLAG
    );
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={DISPLAY_NAME}\n\
         Comment=VoltNest 设备监控（开机自启）\n\
         Exec={exec}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

/// string 类型的值反义：\\ → \、\s → 空格、\n/\t/\r；其它转义原样保留（规范里属无效转义）。
fn unescape_value(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    out
}

/// 取 .desktop 内容里 [Desktop Entry] 组的 Exec 值（已做值级反义，尚未做 Exec 分词）。
///
/// 只认 [Desktop Entry] 组：其它组里的 Exec 与自启无关。
pub fn exec_value(content: &str) -> Option<String> {
    let mut in_entry = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("Exec=") {
            return Some(unescape_value(value.trim_end()));
        }
    }
    None
}

/// Exec 值里的第一个参数（可执行文件路径），按规范去掉引号与转义。
///
/// 只用来回读本模块写出的条目，因此不展开 %f/%u 之类的域代码——本项目从不写它们。
pub fn exec_program(exec: &str) -> Option<String> {
    let chars: Vec<char> = exec.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    let mut out = String::new();
    let mut quoted = false;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted && i < chars.len() => match chars[i] {
                '"' | '\\' | '`' | '$' => {
                    out.push(chars[i]);
                    i += 1;
                }
                _ => out.push('\\'),
            },
            '%' if chars.get(i) == Some(&'%') => {
                out.push('%');
                i += 1;
            }
            c if c.is_whitespace() && !quoted => break,
            c => out.push(c),
        }
    }
    (!out.is_empty()).then_some(out)
}

/// 当前自启命令（Windows：注册表值；Linux：.desktop 条目的 Exec 值）；不存在返回 None。
#[cfg(windows)]
pub fn current() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu.open_subkey(RUN_KEY).ok()?;
    run.get_value::<String, _>(VALUE_NAME).ok()
}

/// Linux：读取自启条目的 Exec 值，条目不存在或不可读返回 None。
#[cfg(target_os = "linux")]
pub fn current() -> Option<String> {
    let text = std::fs::read_to_string(desktop_path()?).ok()?;
    exec_value(&text)
}

/// 其它平台恒为 None。
#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn current() -> Option<String> {
    None
}

/// 是否已启用（Windows）。
#[cfg(windows)]
pub fn is_enabled() -> bool {
    current().is_some()
}

/// 是否已启用（Linux）：条目存在且 Exec 指向当前可执行文件。
///
/// 只判存在会把「装到旧路径的遗留条目」也算成已启用，界面就会说谎：条目在但指向别的可执行文件，
/// 下次登录拉起的是那个程序，不是本进程。
#[cfg(target_os = "linux")]
pub fn is_enabled() -> bool {
    let Some(path) = desktop_path() else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(program) = exec_value(&text).and_then(|value| exec_program(&value)) else {
        return false;
    };
    same_file(Path::new(&program), &exe)
}

/// 两个路径是否指同一个可执行文件：先比字面，再比规范化结果（符号链接安装也算同一文件）。
#[cfg(target_os = "linux")]
fn same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// 是否已启用（其它平台）。
#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn is_enabled() -> bool {
    current().is_some()
}

/// 写入自启项（Windows）。
#[cfg(windows)]
pub fn enable(exe: &Path) -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)?;
    run.set_value(VALUE_NAME, &run_command(exe))
}

/// 写入自启条目（Linux）。
///
/// 先写临时文件再 rename：桌面环境随时可能读这个目录，半截条目会被当成坏条目。
#[cfg(target_os = "linux")]
pub fn enable(exe: &Path) -> std::io::Result<()> {
    let path = desktop_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "无法确定 XDG 配置目录：XDG_CONFIG_HOME 与 HOME 均未设置",
        )
    })?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("desktop.tmp");
    std::fs::write(&tmp, desktop_entry(exe))?;
    std::fs::rename(&tmp, &path)
}

/// 其它平台报错，不静默假装成功。
#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn enable(_exe: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "自启仅支持 Windows 与 Linux",
    ))
}

/// 清除自启项（Windows）；不存在视为成功，卸载清理可重复执行。
#[cfg(windows)]
pub fn disable() -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = match hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE) {
        Ok(k) => k,
        Err(_) => return Ok(()),
    };
    match run.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(std::io::Error::other(e.to_string())),
    }
}

/// 清除自启条目（Linux）；不存在视为成功。
#[cfg(target_os = "linux")]
pub fn disable() -> std::io::Result<()> {
    let Some(path) = desktop_path() else {
        return Ok(());
    };
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// 其它平台空操作。
#[cfg(all(not(windows), not(target_os = "linux")))]
pub fn disable() -> std::io::Result<()> {
    Ok(())
}

/// 当前用户的 XDG 配置根：$XDG_CONFIG_HOME，缺省 $HOME/.config。
///
/// XDG 规范要求 $XDG_CONFIG_HOME 是绝对路径，相对路径按未设置处理。
#[cfg(target_os = "linux")]
fn config_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return Some(dir);
        }
    }
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(".config"))
}

/// 自启条目路径：<配置根>/autostart/voltnest.desktop（纯逻辑，便于测试）。
pub fn desktop_path_in(config_home: Option<&Path>) -> Option<PathBuf> {
    config_home.map(|root| root.join(AUTOSTART_DIR).join(DESKTOP_FILE_NAME))
}

/// 当前用户的自启条目路径。
#[cfg(target_os = "linux")]
pub fn desktop_path() -> Option<PathBuf> {
    desktop_path_in(config_home().as_deref())
}

/// 把实际状态同步到期望状态（双向同步的唯一入口）。
pub fn set_enabled(exe: &Path, enabled: bool) -> std::io::Result<()> {
    if enabled { enable(exe) } else { disable() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_lines(text: &str) -> Vec<&str> {
        text.lines().map(str::trim_end).collect()
    }

    #[test]
    fn command_quotes_paths_with_spaces() {
        let cmd = run_command(&PathBuf::from(r"C:\Program Files\VoltNest\VoltNest.exe"));
        assert_eq!(
            cmd,
            "\"C:\\Program Files\\VoltNest\\VoltNest.exe\" --autostart"
        );
    }

    #[test]
    fn command_without_spaces_stays_unquoted() {
        let cmd = run_command(&PathBuf::from(r"C:\Tools\VoltNest.exe"));
        assert_eq!(cmd, "C:\\Tools\\VoltNest.exe --autostart");
    }

    #[test]
    fn command_always_carries_autostart_flag() {
        // 命令恒带 --autostart，首屏显隐另由「启动时最小化」决定。
        for p in [r"C:\a b\VoltNest.exe", r"C:\a\VoltNest.exe"] {
            assert!(run_command(&PathBuf::from(p)).ends_with("--autostart"));
        }
    }

    #[test]
    fn trailing_backslash_does_not_escape_quote() {
        let cmd = run_command(&PathBuf::from("C:\\My Dir\\"));
        assert!(!cmd.contains("\\\\\""), "反斜杠转义了引号：{cmd}");
        assert!(cmd.starts_with('"'));
    }

    #[test]
    fn run_key_matches_microsoft_location() {
        assert_eq!(RUN_KEY, r"Software\Microsoft\Windows\CurrentVersion\Run");
        assert_eq!(VALUE_NAME, "VoltNest");
    }

    #[test]
    fn desktop_entry_has_required_keys() {
        let entry = desktop_entry(Path::new("/opt/voltnest/voltnest"));
        let lines = entry_lines(&entry);
        assert_eq!(lines[0], "[Desktop Entry]");
        for key in [
            "Type=Application",
            "X-GNOME-Autostart-enabled=true",
            "Terminal=false",
            "Exec=/opt/voltnest/voltnest --autostart",
        ] {
            assert!(lines.contains(&key), "缺少 {key}：{lines:?}");
        }
        assert!(lines.contains(&"Name=VoltNest"));
    }

    #[test]
    fn exec_path_with_space_is_quoted() {
        let entry = desktop_entry(Path::new("/opt/Volt Nest/voltnest"));
        let lines = entry_lines(&entry);
        assert!(
            lines.contains(&"Exec=\"/opt/Volt Nest/voltnest\" --autostart"),
            "{lines:?}"
        );
    }

    #[test]
    fn exec_percent_is_escaped_as_field_code() {
        // 未转义的 % 会被桌面环境当成域代码（%f/%u）而吞掉。
        let entry = desktop_entry(Path::new("/opt/50%/voltnest"));
        assert_eq!(
            exec_value(&entry).as_deref(),
            Some("/opt/50%%/voltnest --autostart")
        );
    }

    #[test]
    fn exec_round_trips_tricky_paths() {
        for path in [
            "/opt/voltnest/voltnest",
            "/opt/Volt Nest/voltnest",
            "/opt/a\\tb/voltnest",
            "/opt/50%/voltnest",
            "/opt/`whoami`/voltnest",
            "/opt/do\"llar$/voltnest",
            "/opt/back\\slash/voltnest",
        ] {
            let entry = desktop_entry(Path::new(path));
            let value = exec_value(&entry).expect("条目里应有 Exec");
            assert_eq!(exec_program(&value).as_deref(), Some(path), "{entry}");
        }
    }

    #[test]
    fn exec_program_reads_hand_written_unquoted_entry() {
        assert_eq!(
            exec_program("/usr/bin/voltnest --autostart").as_deref(),
            Some("/usr/bin/voltnest")
        );
        assert_eq!(
            exec_program("\"/usr/bin/volt nest\" --autostart").as_deref(),
            Some("/usr/bin/volt nest")
        );
        assert_eq!(exec_program("   ").as_deref(), None);
        assert_eq!(exec_program("").as_deref(), None);
    }

    #[test]
    fn exec_value_ignores_other_groups() {
        let text =
            "[Desktop Action new]\nExec=/usr/bin/other\n[Desktop Entry]\nExec=/usr/bin/voltnest\n";
        assert_eq!(exec_value(text).as_deref(), Some("/usr/bin/voltnest"));
        assert_eq!(
            exec_value("[Desktop Action new]\nExec=/usr/bin/other\n").as_deref(),
            None
        );
    }

    #[test]
    fn entry_written_to_disk_reads_back() {
        // 真实落盘一轮：写入 → 读回 → Exec 仍指向同一个可执行文件。
        let dir = std::env::temp_dir().join(format!("voltnest-autostart-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(DESKTOP_FILE_NAME);
        let exe = Path::new("/opt/Volt Nest/voltnest");
        std::fs::write(&file, desktop_entry(exe)).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let program = exec_value(&text).and_then(|value| exec_program(&value));
        assert_eq!(program.as_deref(), Some("/opt/Volt Nest/voltnest"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn desktop_path_follows_xdg_layout() {
        let path = desktop_path_in(Some(Path::new("/home/u/.config")));
        assert_eq!(
            path,
            Some(PathBuf::from("/home/u/.config/autostart/voltnest.desktop"))
        );
        assert_eq!(desktop_path_in(None), None);
    }
}

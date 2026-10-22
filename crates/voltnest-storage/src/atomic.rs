//! 原子写入：`.tmp` → `fsync` → `rename`（X-2）。
//!
//! 禁止「试写文件探测权限」式写法：那会被杀软当成可疑行为，而且它只在写入时
//! 才发现失败，等于把问题推迟到用户已经改了设置之后。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 写 `.tmp` 文件：与目标同目录（保证 `rename` 不跨卷），先 `truncate` 再写。
fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// 原子写入。
///
/// 半写状态只会出现在 `.tmp` 里，`rename` 之后目标文件要么是旧内容、要么是新内容。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(path);
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.flush()?;
        // fsync：断电后 rename 不会把空文件留在目标位置。
        file.sync_all()?;
    }
    // Windows 的 rename 走 MOVEFILE_REPLACE_EXISTING，可直接覆盖。
    fs::rename(&tmp, path)
}

/// 原子写入 JSON（缩进 2 空格，方便用户排查与手工修复）。
pub fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_json::to_vec_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_atomic(path, &text)
}

/// 把损坏文件改名存档，返回存档路径。目标已存在时追加序号，绝不覆盖已有存档。
pub fn archive_corrupt(from: &Path, to: &Path) -> io::Result<PathBuf> {
    let mut target = to.to_path_buf();
    let mut seq = 1u32;
    while target.exists() {
        let stem = to.file_stem().and_then(|s| s.to_str()).unwrap_or("corrupt");
        let ext = to.extension().and_then(|s| s.to_str()).unwrap_or("json");
        target = to.with_file_name(format!("{stem}-{seq}.{ext}"));
        seq += 1;
        if seq > 100 {
            break;
        }
    }
    fs::rename(from, &target)?;
    Ok(target)
}

/// 读文件；不存在返回 `Ok(None)`（`state.json` 允许随时删除，X-5）。
pub fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match File::open(path) {
        Ok(mut f) => {
            let mut buf = Vec::new();
            io::Read::read_to_end(&mut f, &mut buf)?;
            Ok(Some(buf))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("voltnest-test-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_and_leaves_no_tmp_file() {
        let dir = temp_dir("atomic");
        let path = dir.join("settings.json");
        write_atomic(&path, b"{\"a\":1}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"a\":1}");
        assert!(!tmp_path(&path).exists(), "临时文件必须被 rename 掉");
    }

    #[test]
    fn overwrites_existing_file() {
        let dir = temp_dir("overwrite");
        let path = dir.join("state.json");
        write_atomic(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn creates_parent_directories() {
        let dir = temp_dir("nested");
        let path = dir.join("a/b/c.json");
        write_atomic(&path, b"1").unwrap();
        assert!(path.exists());
    }

    #[test]
    fn read_optional_handles_missing_file() {
        let dir = temp_dir("missing");
        assert!(read_optional(&dir.join("nope.json")).unwrap().is_none());
    }

    #[test]
    fn archive_never_overwrites_previous_archive() {
        let dir = temp_dir("archive");
        let from = dir.join("settings.json");
        fs::write(&from, b"bad").unwrap();
        let first = archive_corrupt(&from, &dir.join("settings.corrupt-1.json")).unwrap();
        assert_eq!(first.file_name().unwrap(), "settings.corrupt-1.json");

        fs::write(&from, b"bad2").unwrap();
        let second = archive_corrupt(&from, &dir.join("settings.corrupt-1.json")).unwrap();
        assert_eq!(second.file_name().unwrap(), "settings.corrupt-1-1.json");
        assert_eq!(fs::read(&first).unwrap(), b"bad", "第一份存档不得被覆盖");
    }

    #[test]
    fn json_writer_pretty_prints() {
        let dir = temp_dir("json");
        let path = dir.join("x.json");
        write_json_atomic(&path, &serde_json::json!({"a":1})).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\n"), "应缩进便于手工修复：{text}");
    }
}

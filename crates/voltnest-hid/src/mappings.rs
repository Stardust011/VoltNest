//! 型号表目录：**运行时**从文件系统定位并加载 `data/mappings/*.json`。
//!
//! 型号表不再用 `include_str!` 编译进二进制：进程启动时遍历型号表目录，把每份 `*.json`
//! 解析成 [`DeviceTable`]，再按 [`crate::spec::BrandSpec::device_table_file`] 交给对应品牌。
//! 换一份型号表因此不必重编二进制，代价是安装包必须带上该目录 —— 目录定位是全 crate 唯一的
//! 环境相关逻辑，集中在这里，按固定优先级尝试：
//!
//! 1. `VOLTNEST_HID_MAPPINGS_DIR`：显式指定，**一旦设置即为权威**（指错直接报错，不静默回落）。
//! 2. 可执行文件同级（安装形态）：`<exe>/mappings`、`<exe>/data/mappings`、`<exe>/resources/mappings`
//!    （Windows NSIS / 免安装版；Tauri 的 `bundle.resources` 落在 resource 目录，Windows 上就是 exe 所在目录）。
//! 3. 可执行文件上一级（`bin/` 布局）：`<exe>/../Resources/mappings`（macOS `.app`）、`<exe>/../mappings`、
//!    `<exe>/../data/mappings`、`<exe>/../share/voltnest/mappings`，以及 Linux 安装形态
//!    `<exe>/../lib/<任意子目录>/mappings`（deb / AppImage 的 Tauri 资源目录是 `lib/<productName>`）。
//! 4. 源码树（开发期 `cargo run` / `cargo test`）：`<CARGO_MANIFEST_DIR>/data/mappings`。
//!
//! 目录必须存在且至少含一份 `*.json`：候选目录「存在且有表」即采用；全部不成立时报
//! [`MappingError::DirNotFound`]，并把**所有已尝试路径**写进错误信息 —— 装错包时这行日志就是全部线索。
//!
//! 目录内每份 `*.json` 都会被解析（隐藏文件与非 `.json` 后缀忽略）。**单份表读不出或解析失败
//! 只跳过这一份并记入 [`MappingSet::skipped`]，不拖垮整次启动** —— 一份坏表不该让其它品牌的设备全部消失；
//! 反过来也不能「静默」：跳过项随 [`crate::brands::Registry::skipped`] 一并暴露，由调用方展示。
//! 「一份表都没加载到」仍由注册表判定为部署事故（`RegistryError::NoUsableBrand`），因为那不是局部损坏。

use crate::table::DeviceTable;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// 指定型号表目录的环境变量。设置后即为权威路径，不再回落到其它候选。
pub const DIR_ENV: &str = "VOLTNEST_HID_MAPPINGS_DIR";

/// 型号表目录名。
pub const DIR_NAME: &str = "mappings";

/// 型号表定位 / 加载失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MappingError {
    /// 所有候选目录都不存在（或都不含型号表）。
    DirNotFound {
        /// 按优先级尝试过的路径。
        candidates: Vec<PathBuf>,
    },
    /// 目录存在但读不了（权限 / 不是目录）。
    DirUnreadable {
        /// 目录路径。
        dir: PathBuf,
        /// 系统给出的原因。
        detail: String,
    },
    /// 目录里一份型号表都没有。
    Empty {
        /// 目录路径。
        dir: PathBuf,
    },
    /// 单份型号表读不出来。
    FileUnreadable {
        /// 文件路径。
        path: PathBuf,
        /// 系统给出的原因。
        detail: String,
    },
    /// 单份型号表内容非法（JSON 结构 / 键格式 / 空表）。
    InvalidTable {
        /// 文件路径。
        path: PathBuf,
        /// 解析器给出的原因。
        detail: String,
    },
    /// 目录里没有品牌声明的那份型号表。
    MissingTable {
        /// 目录路径。
        dir: PathBuf,
        /// 品牌声明的文件名。
        file: String,
        /// 目录里实际存在的文件名（升序）。
        available: Vec<String>,
    },
}

impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirNotFound { candidates } => {
                write!(f, "未找到型号表目录（已尝试：")?;
                for (i, path) in candidates.iter().enumerate() {
                    if i > 0 {
                        write!(f, "、")?;
                    }
                    write!(f, "{}", path.display())?;
                }
                write!(f, "）；可用环境变量 {DIR_ENV} 指定")
            }
            Self::DirUnreadable { dir, detail } => {
                write!(f, "型号表目录 {} 无法读取：{detail}", dir.display())
            }
            Self::Empty { dir } => {
                write!(f, "型号表目录 {} 内没有任何 *.json 型号表", dir.display())
            }
            Self::FileUnreadable { path, detail } => {
                write!(f, "型号表 {} 无法读取：{detail}", path.display())
            }
            Self::InvalidTable { path, detail } => {
                write!(f, "型号表 {} 解析失败：{detail}", path.display())
            }
            Self::MissingTable {
                dir,
                file,
                available,
            } => write!(
                f,
                "型号表目录 {} 内缺少 {file}（现有：{}）",
                dir.display(),
                if available.is_empty() {
                    "无".to_string()
                } else {
                    available.join("、")
                }
            ),
        }
    }
}

impl std::error::Error for MappingError {}

/// 按优先级列出型号表目录的候选路径（只列出，不校验是否存在）。
///
/// 顺序即优先级；重复路径只保留首次出现。`<exe>/../lib/*` 一项需要读一层目录列举，
/// 其余均为纯拼路径。
pub fn candidate_dirs() -> Vec<PathBuf> {
    candidate_dirs_for(current_exe_dir().as_deref(), env_dir().as_deref())
}

/// 候选路径的生成函数：`exe_dir` 可为 `None`（拿不到可执行文件路径时）。
fn candidate_dirs_for(exe_dir: Option<&Path>, explicit: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        let path = normalize(&path);
        if !dirs.contains(&path) {
            dirs.push(path);
        }
    };

    if let Some(explicit) = explicit {
        push(explicit.to_path_buf());
    }

    if let Some(exe_dir) = exe_dir {
        push(exe_dir.join(DIR_NAME));
        push(exe_dir.join("data").join(DIR_NAME));
        push(exe_dir.join("resources").join(DIR_NAME));
        let up = exe_dir.join("..");
        push(up.join("Resources").join(DIR_NAME));
        push(up.join(DIR_NAME));
        push(up.join("data").join(DIR_NAME));
        push(up.join("share").join("voltnest").join(DIR_NAME));
        for dir in lib_layout_dirs(exe_dir) {
            push(dir);
        }
    }

    push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join(DIR_NAME),
    );

    dirs
}

/// `<exe>/../lib/<子目录>/mappings`：目录名由打包器的 productName 决定，故扫一层子目录而不硬编码。
fn lib_layout_dirs(exe_dir: &Path) -> Vec<PathBuf> {
    let lib = normalize(&exe_dir.join("..").join("lib"));
    let Ok(entries) = std::fs::read_dir(&lib) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path().join(DIR_NAME))
        .filter(|dir| dir.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// 定位型号表目录：第一个「存在且含至少一份型号表」的候选。
///
/// `VOLTNEST_HID_MAPPINGS_DIR` 一旦设置就只认它（即使是空的或不存在），因为「指了却静默用了别的目录」
/// 会让排查方向彻底跑偏。
pub fn resolve_dir() -> Result<PathBuf, MappingError> {
    resolve_in(current_exe_dir().as_deref(), env_dir().as_deref())
}

/// 定位的纯函数版本：显式目录优先且具有排他性，其余候选按 [`candidate_dirs_for`] 的顺序取第一个可用者。
fn resolve_in(exe_dir: Option<&Path>, explicit: Option<&Path>) -> Result<PathBuf, MappingError> {
    if let Some(explicit) = explicit {
        let explicit = normalize(explicit);
        if !explicit.is_dir() {
            return Err(MappingError::DirNotFound {
                candidates: vec![explicit],
            });
        }
        return if has_table(&explicit) {
            Ok(explicit)
        } else {
            Err(MappingError::Empty { dir: explicit })
        };
    }
    let candidates = candidate_dirs_for(exe_dir, None);
    for dir in &candidates {
        if dir.is_dir() && has_table(dir) {
            return Ok(dir.clone());
        }
    }
    Err(MappingError::DirNotFound { candidates })
}

/// 词法规整路径：消掉 `.` 与 `..`（不访问文件系统）。
///
/// 候选路径全是「exe 目录 + 固定后缀」拼出来的，词法退一级即可得到真实位置；保留 `..` 会让
/// 诊断日志里的路径无法直接复制核对。`canonicalize` 不用：Windows 上会加 `\\?\` 前缀。
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `VOLTNEST_HID_MAPPINGS_DIR` 的值（未设置或为空串时为 `None`）。
fn env_dir() -> Option<PathBuf> {
    std::env::var_os(DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// 当前可执行文件所在目录（多平台一致；拿不到时为 `None`）。
fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

/// 被跳过的一份型号表（读不出或解析失败），连同原因。
///
/// 存在的意义是「跳过但留证」：设备只会少一批，日志与诊断页必须能说清少的是哪份表、为什么。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedTable {
    /// 文件名（如 `ajazz.json`）。
    pub file: String,
    /// 跳过原因（读取失败 / 结构非法）。
    pub error: MappingError,
}

impl fmt::Display for SkippedTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "型号表 {} 已跳过：{}", self.file, self.error)
    }
}

/// 读入整份型号表目录。返回按文件名索引的集合，供注册表按品牌分派。
///
/// 坏表不阻断加载：解析失败的条目进 [`MappingSet::skipped`]，其余照常可用。
#[derive(Debug)]
pub struct MappingSet {
    dir: PathBuf,
    tables: HashMap<String, DeviceTable>,
    skipped: Vec<SkippedTable>,
}

impl MappingSet {
    /// 定位目录并加载其中全部型号表。
    pub fn load() -> Result<Self, MappingError> {
        let dir = resolve_dir()?;
        Self::load_from(&dir)
    }

    /// 加载指定目录下的全部型号表（跳过隐藏文件与非 `*.json`）。
    pub fn load_from(dir: &Path) -> Result<Self, MappingError> {
        let entries = std::fs::read_dir(dir).map_err(|e| MappingError::DirUnreadable {
            dir: dir.to_path_buf(),
            detail: e.to_string(),
        })?;
        let mut paths: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| MappingError::DirUnreadable {
                dir: dir.to_path_buf(),
                detail: e.to_string(),
            })?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.') || !is_table_file(&path) {
                continue;
            }
            paths.push((name.to_string(), path));
        }
        paths.sort_by(|a, b| a.0.cmp(&b.0));
        if paths.is_empty() {
            return Err(MappingError::Empty {
                dir: dir.to_path_buf(),
            });
        }
        let mut tables = HashMap::with_capacity(paths.len());
        let mut skipped = Vec::new();
        for (name, path) in paths {
            match read_table(&path) {
                Ok(table) => {
                    tables.insert(name, table);
                }
                Err(error) => skipped.push(SkippedTable { file: name, error }),
            }
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            tables,
            skipped,
        })
    }

    /// 实际使用的目录（诊断用）。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 取某个品牌声明的型号表（按文件名，大小写敏感）。每个品牌各自持有一份副本。
    ///
    /// 文件存在但加载失败时返回**真实原因**（解析失败 / 读不出），而不是笼统的「缺少该文件」——
    /// 否则日志会指向一个明明就在盘上的文件，排查方向直接跑偏。
    pub fn table(&self, file: &str) -> Result<DeviceTable, MappingError> {
        if let Some(table) = self.tables.get(file) {
            return Ok(table.clone());
        }
        if let Some(skipped) = self.skipped.iter().find(|item| item.file == file) {
            return Err(skipped.error.clone());
        }
        Err(MappingError::MissingTable {
            dir: self.dir.clone(),
            file: file.to_string(),
            available: self.files().iter().map(|s| (*s).to_string()).collect(),
        })
    }

    /// 已加载的文件名（升序）。
    pub fn files(&self) -> Vec<&str> {
        let mut files: Vec<&str> = self.tables.keys().map(String::as_str).collect();
        files.sort_unstable();
        files
    }

    /// 已加载型号表数量。
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// 被跳过的型号表（按文件名升序）。空表示全部加载成功。
    pub fn skipped(&self) -> &[SkippedTable] {
        &self.skipped
    }

    /// 是否一份都没加载到（`len()` 的配套方法；构造时已拒绝空目录，正常恒 `false`）。
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

/// 只加载一份型号表（不扫描整个目录）：先定位目录，再按文件名读取。
///
/// 供单元测试与「单独校验某个品牌型号表」的场景使用；注册表走 [`MappingSet`]，
/// 一次扫描即可覆盖全部品牌。
pub fn load_table(file: &str) -> Result<DeviceTable, MappingError> {
    let dir = resolve_dir()?;
    read_table(&dir.join(file))
}

/// 目录里是否有至少一份型号表。
fn has_table(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                let path = entry.path();
                is_table_file(&path)
                    && !path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with('.'))
            })
        })
        .unwrap_or(false)
}

/// 扩展名恰为 `.json`。
fn is_table_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

/// 读一个文件并解析成型号表。
fn read_table(path: &Path) -> Result<DeviceTable, MappingError> {
    let text = std::fs::read_to_string(path).map_err(|e| MappingError::FileUnreadable {
        path: path.to_path_buf(),
        detail: e.to_string(),
    })?;
    DeviceTable::parse(&text).map_err(|detail| MappingError::InvalidTable {
        path: path.to_path_buf(),
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实安装形态：`data/mappings` 里的四份表必须都能被本模块读到并加载。
    #[test]
    fn loads_checked_in_tables() {
        let set = MappingSet::load().expect("源码树内必须能定位型号表目录");
        assert!(set.dir().ends_with(DIR_NAME), "目录：{:?}", set.dir());
        for file in ["ajazz.json", "dareu.json", "mchose.json", "rapoo.json"] {
            let table = set.table(file).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(!table.is_empty(), "{file} 不得为空表");
        }
        assert_eq!(set.len(), 4);
        assert_eq!(
            set.files(),
            vec!["ajazz.json", "dareu.json", "mchose.json", "rapoo.json"]
        );
    }

    /// 缺文件必须报出「现有哪几份」，而不是笼统的 IO 错误。
    #[test]
    fn missing_file_lists_available_tables() {
        let set = MappingSet::load().unwrap();
        let err = set.table("nope.json").unwrap_err();
        match &err {
            MappingError::MissingTable {
                file, available, ..
            } => {
                assert_eq!(file, "nope.json");
                assert!(available.iter().any(|f| f == "ajazz.json"));
            }
            other => panic!("期望 MissingTable，实际 {other:?}"),
        }
        assert!(err.to_string().contains("ajazz.json"), "{err}");
    }

    /// 坏表只跳过自己并记录原因，**不**让整次加载失败：其它品牌的设备必须照常采集。
    #[test]
    fn invalid_table_is_skipped_and_recorded() {
        let dir = temp_dir("invalid");
        std::fs::write(dir.join("bad.json"), "{ not json").unwrap();
        write_ajazz(&dir);
        let set = MappingSet::load_from(&dir).unwrap();
        assert_eq!(set.len(), 1, "好表必须照常加载");
        assert_eq!(set.files(), vec!["ajazz.json"]);
        assert_eq!(set.skipped().len(), 1);
        let skip = &set.skipped()[0];
        assert_eq!(skip.file, "bad.json");
        assert!(
            matches!(&skip.error, MappingError::InvalidTable { path, .. } if path.ends_with("bad.json")),
            "{skip:?}"
        );
        assert_eq!(
            skip.to_string(),
            format!("型号表 bad.json 已跳过：{}", skip.error)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 文件在盘上但解析失败：`table()` 必须报**解析失败**，不能报「缺少该文件」。
    #[test]
    fn table_reports_parse_failure_not_missing() {
        let dir = temp_dir("parse-fail");
        std::fs::write(dir.join("bad.json"), "{ not json").unwrap();
        let set = MappingSet::load_from(&dir).unwrap();
        assert!(
            matches!(
                set.table("bad.json"),
                Err(MappingError::InvalidTable { .. })
            ),
            "{:?}",
            set.table("bad.json")
        );
        assert!(matches!(
            set.table("absent.json"),
            Err(MappingError::MissingTable { .. })
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 名为 `*.json` 的**目录**不是型号表：忽略（既不加载，也不计入跳过项）。
    #[test]
    fn json_named_directory_is_ignored() {
        let dir = temp_dir("json-dir");
        write_ajazz(&dir);
        std::fs::create_dir(dir.join("broken.json")).unwrap();
        let set = MappingSet::load_from(&dir).unwrap();
        assert_eq!(set.files(), vec!["ajazz.json"]);
        assert!(set.skipped().is_empty(), "{:?}", set.skipped());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 空目录（或只有非 JSON 文件）报 `Empty`，不得当成「没有品牌」静默通过。
    #[test]
    fn empty_dir_is_an_error() {
        let dir = temp_dir("empty");
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert!(matches!(
            MappingSet::load_from(&dir),
            Err(MappingError::Empty { .. })
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 隐藏文件与非 `.json` 后缀不得参与加载（编辑器临时文件不该让启动失败）。
    #[test]
    fn skips_hidden_and_non_json_files() {
        let dir = temp_dir("skip");
        let json = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("data")
                .join(DIR_NAME)
                .join("ajazz.json"),
        )
        .unwrap();
        std::fs::write(dir.join("ajazz.json"), json).unwrap();
        std::fs::write(dir.join(".ajazz.json.swp"), "garbage").unwrap();
        std::fs::write(dir.join("notes.txt"), "garbage").unwrap();
        let set = MappingSet::load_from(&dir).unwrap();
        assert_eq!(set.files(), vec!["ajazz.json"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 候选路径必须覆盖「exe 同级 / 上一级 / 源码树」三类布局，且无重复。
    #[test]
    fn candidates_cover_install_and_dev_layouts() {
        let dirs = candidate_dirs();
        let joined: Vec<String> = dirs.iter().map(|p| p.display().to_string()).collect();
        assert!(
            joined
                .iter()
                .any(|p| p.ends_with("data\\mappings") || p.ends_with("data/mappings")),
            "{joined:?}"
        );
        assert!(dirs.iter().all(|p| p.ends_with(DIR_NAME)), "{joined:?}");
        let mut unique = dirs.clone();
        unique.dedup();
        assert_eq!(unique.len(), dirs.len(), "候选不得重复：{joined:?}");
    }

    /// Windows 安装形态（NSIS / 免安装）：`<exe>/mappings/*.json` 必须被直接命中。
    #[test]
    fn resolves_layout_next_to_exe() {
        let exe = temp_dir("layout-exe");
        let dir = exe.join(DIR_NAME);
        std::fs::create_dir_all(&dir).unwrap();
        write_ajazz(&dir);
        assert_eq!(resolve_in(Some(&exe), None).unwrap(), dir);
        std::fs::remove_dir_all(&exe).ok();
    }

    /// 数据子目录形态：`<exe>/data/mappings/*.json`（exe 旁没有 `mappings/` 时降级到它）。
    #[test]
    fn resolves_layout_under_data_dir() {
        let exe = temp_dir("layout-data");
        let dir = exe.join("data").join(DIR_NAME);
        std::fs::create_dir_all(&dir).unwrap();
        write_ajazz(&dir);
        assert_eq!(resolve_in(Some(&exe), None).unwrap(), dir);
        std::fs::remove_dir_all(&exe).ok();
    }

    /// Linux 安装形态：`<prefix>/bin/<exe>` + `<prefix>/lib/<productName>/mappings`。
    #[test]
    fn resolves_layout_under_lib_dir() {
        let prefix = temp_dir("layout-lib");
        let exe = prefix.join("bin");
        let dir = prefix.join("lib").join("VoltNest").join(DIR_NAME);
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        write_ajazz(&dir);
        assert_eq!(resolve_in(Some(&exe), None).unwrap(), dir);
        std::fs::remove_dir_all(&prefix).ok();
    }

    /// `lib/` 下没有可用型号表时不得给出候选：`<exe>/../lib` 存在但为空目录要照常跳过。
    #[test]
    fn empty_lib_dir_yields_no_candidates() {
        let prefix = temp_dir("layout-lib-empty");
        let exe = prefix.join("bin");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::create_dir_all(prefix.join("lib").join("VoltNest")).unwrap();
        assert!(lib_layout_dirs(&exe).is_empty());
        std::fs::remove_dir_all(&prefix).ok();
    }

    /// macOS `.app` 形态：`Contents/MacOS/<exe>` → `Contents/Resources/mappings`。
    #[test]
    fn resolves_layout_of_macos_app_bundle() {
        let contents = temp_dir("layout-macos");
        let macos = contents.join("MacOS");
        let resources = contents.join("Resources").join(DIR_NAME);
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(&resources).unwrap();
        write_ajazz(&resources);
        assert_eq!(resolve_in(Some(&macos), None).unwrap(), resources);
        std::fs::remove_dir_all(&contents).ok();
    }

    /// 显式目录具有排他性：指到空目录必须报 `Empty`，绝不静默改用 exe 旁边那份表。
    #[test]
    fn explicit_dir_is_authoritative() {
        let exe = temp_dir("authority-exe");
        let install = exe.join(DIR_NAME);
        std::fs::create_dir_all(&install).unwrap();
        write_ajazz(&install);

        let empty = temp_dir("authority-empty");
        assert_eq!(
            resolve_in(Some(&exe), Some(&empty)),
            Err(MappingError::Empty { dir: empty.clone() })
        );

        let missing = empty.join("nope");
        assert!(matches!(
            resolve_in(Some(&exe), Some(&missing)),
            Err(MappingError::DirNotFound { .. })
        ));
        std::fs::remove_dir_all(&exe).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    /// exe 旁边没有表时降级到源码树：开发期 `cargo run` / `cargo test` 全靠这条。
    ///
    /// 同时验证候选列表「exe 同级 → 源码树」的完整顺序（装错包时错误信息就是照它列出来的）。
    #[test]
    fn falls_back_to_source_tree_in_dev() {
        let exe = temp_dir("fallback");
        let candidates = candidate_dirs_for(Some(&exe), None);
        assert_eq!(candidates.first(), Some(&exe.join(DIR_NAME)));
        let dir = resolve_in(Some(&exe), None).unwrap();
        assert!(
            dir.ends_with("data\\mappings") || dir.ends_with("data/mappings"),
            "{dir:?}"
        );
        assert!(dir.join("ajazz.json").is_file());
        std::fs::remove_dir_all(&exe).ok();
    }

    /// 把源码树里的真实型号表复制到指定目录（构造「安装形态」用）。
    fn write_ajazz(dir: &Path) {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join(DIR_NAME)
            .join("ajazz.json");
        std::fs::copy(src, dir.join("ajazz.json")).unwrap();
    }

    /// 每个测试用独立目录，避免并行跑测试时互相删除。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voltnest-hid-mappings-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            std::fs::remove_file(entry.path()).ok();
        }
        dir
    }

    /// 装错包时这行日志就是全部线索：每个变体都要能说清路径与原因。
    #[test]
    fn mapping_error_display_covers_every_variant() {
        let dir = PathBuf::from("/opt/voltnest/mappings");
        let file = dir.join("ajazz.json");
        let cases = vec![
            (
                MappingError::DirNotFound {
                    candidates: vec![dir.clone(), PathBuf::from("/usr/share/voltnest/mappings")],
                },
                "/usr/share/voltnest/mappings",
            ),
            (
                MappingError::DirUnreadable {
                    dir: dir.clone(),
                    detail: "拒绝访问".to_string(),
                },
                "拒绝访问",
            ),
            (MappingError::Empty { dir: dir.clone() }, "没有任何 *.json"),
            (
                MappingError::FileUnreadable {
                    path: file.clone(),
                    detail: "不是 UTF-8".to_string(),
                },
                "不是 UTF-8",
            ),
            (
                MappingError::InvalidTable {
                    path: file.clone(),
                    detail: "缺少 devices".to_string(),
                },
                "缺少 devices",
            ),
            (
                MappingError::MissingTable {
                    dir: dir.clone(),
                    file: "rapoo.json".to_string(),
                    available: Vec::new(),
                },
                "现有：无",
            ),
        ];
        for (err, needle) in cases {
            let text = err.to_string();
            assert!(
                text.contains(needle),
                "{err:?} 的诊断文本缺少 {needle}：{text}"
            );
        }
        let err = MappingError::DirNotFound {
            candidates: vec![dir.clone()],
        };
        assert!(err.to_string().contains(DIR_ENV), "{err}");
    }

    /// 正常加载时不该被判成空表，且要报出实际使用的目录。
    #[test]
    fn loaded_set_reports_dir_and_is_not_empty() {
        let set = MappingSet::load().unwrap();
        assert!(!set.is_empty());
        assert!(set.dir().ends_with(DIR_NAME), "目录：{:?}", set.dir());
        assert!(!set.files().is_empty());
    }
}

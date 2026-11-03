//! 设备管理命令：添加、删除、合并、分离、改名。
//!
//! 全部走「读设置 → 校验 → 落盘 → 同步状态机 → Wake」这一条路径，
//! 前端随后调用 `get_snapshot` 即可拿到即时一致的结果（卡片与托盘同源）。

use std::sync::Arc;

use tauri::State;
use voltnest_provider::{StableId, clock};

use crate::dto::DeviceCandidateDto;
use crate::host::{HostEvent, HostState, lock};

/// 合并卡 ID 前缀（与 `voltnest_core` 生成规则一致）。
const CARD_PREFIX: &str = "user:";

/// 「添加设备」的候选：与自动识别共用同一套筛选。
#[tauri::command]
pub fn list_device_candidates(
    shared: State<'_, Arc<HostState>>,
) -> Result<Vec<DeviceCandidateDto>, String> {
    candidates(&shared)
}

/// 候选列表的实现：命令与「添加」校验共用。
fn candidates(shared: &HostState) -> Result<Vec<DeviceCandidateDto>, String> {
    let (removed, aliases) = {
        let settings = lock(&shared.settings);
        (
            settings.devices.removed.clone(),
            settings.devices.aliases.clone(),
        )
    };
    let known: Vec<(String, String, String)> = {
        let engine = lock(&shared.engine);
        engine
            .records()
            .iter()
            .map(|r| {
                (
                    r.stable_id.as_str().to_string(),
                    r.display_name.clone(),
                    r.transport.as_str().to_string(),
                )
            })
            .collect()
    };

    let mut out: Vec<DeviceCandidateDto> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    match voltnest_hid::runtime::list_candidates() {
        Ok(list) => {
            for item in list {
                let in_list = known.iter().any(|(id, _, _)| id == &item.stable_id);
                let display_name = aliases
                    .get(&item.stable_id)
                    .cloned()
                    .unwrap_or(item.display_name);
                seen.push(item.stable_id.clone());
                out.push(DeviceCandidateDto {
                    removed: removed.contains(&item.stable_id),
                    stable_id: item.stable_id,
                    display_name,
                    transport: "hid".to_string(),
                    vid_pid: Some(item.vid_pid),
                    in_list,
                });
            }
        }
        Err(err) => log::warn!("枚举候选设备失败：{err}"),
    }

    // 已知但当前枚举不到的设备（蓝牙、已拔掉）：只在被删除时给一个恢复入口。
    for (id, name, transport) in known {
        if seen.contains(&id) || !removed.contains(&id) {
            continue;
        }
        let display_name = aliases.get(&id).cloned().unwrap_or(name);
        out.push(DeviceCandidateDto {
            stable_id: id,
            display_name,
            transport,
            vid_pid: None,
            in_list: false,
            removed: true,
        });
    }

    out.sort_by(|a, b| {
        a.removed
            .cmp(&b.removed)
            .then_with(|| a.display_name.cmp(&b.display_name))
            .then_with(|| a.stable_id.cmp(&b.stable_id))
    });
    Ok(out)
}

/// 添加（或恢复）一台设备。
#[tauri::command]
pub fn add_device(stable_id: String, shared: State<'_, Arc<HostState>>) -> Result<(), String> {
    let Some(id) = StableId::parse(&stable_id) else {
        return Err(format!("无法识别的设备标识：{stable_id}"));
    };
    let removed = lock(&shared.settings).devices.is_removed(&stable_id);
    let present = {
        let engine = lock(&shared.engine);
        engine.record(&stable_id).is_some()
    };
    if present && !removed {
        return Err("该设备已在列表中".to_string());
    }

    if !present {
        let candidate = candidates(&shared)?
            .into_iter()
            .find(|c| c.stable_id == stable_id && !c.removed);
        let Some(candidate) = candidate else {
            return Err("该设备未接入，或不在自动识别的范围内".to_string());
        };
        lock(&shared.engine).register(&id, &candidate.display_name, clock::now_utc_ms());
    }

    if removed {
        commit(&shared, |devices| {
            devices.restore(&stable_id);
        })?;
    }
    notify_wake(&shared);
    Ok(())
}

/// 删除一批设备（合并卡删除时传它的全部成员）。
#[tauri::command]
pub fn remove_devices(ids: Vec<String>, shared: State<'_, Arc<HostState>>) -> Result<(), String> {
    if ids.is_empty() {
        return Err("没有指定要删除的设备".to_string());
    }
    {
        let engine = lock(&shared.engine);
        for id in &ids {
            if engine.record(id).is_none() {
                return Err(format!("未知设备：{id}"));
            }
        }
    }
    commit(&shared, |devices| devices.remove(&ids))?;
    notify_wake(&shared);
    Ok(())
}

/// 合并：`primary` 为卡片锚点，`members` 为其余设备。
#[tauri::command]
pub fn merge_devices(
    primary: String,
    members: Vec<String>,
    name: String,
    shared: State<'_, Arc<HostState>>,
) -> Result<(), String> {
    if members.iter().any(|id| id == &primary) {
        return Err("不能把设备与自身合并".to_string());
    }
    {
        let engine = lock(&shared.engine);
        if engine.record(&primary).is_none() {
            return Err(format!("未知设备：{primary}"));
        }
        for id in &members {
            if engine.record(id).is_none() {
                return Err(format!("未知设备：{id}"));
            }
        }
    }
    let fallback = {
        let engine = lock(&shared.engine);
        engine
            .record(&primary)
            .map(|r| r.display_name.clone())
            .unwrap_or_else(|| primary.clone())
    };
    let title = if name.trim().is_empty() {
        fallback
    } else {
        name
    };
    commit_try(&shared, |devices| devices.merge(&primary, &members, &title))?;
    notify_wake(&shared);
    Ok(())
}

/// 分离：`card_id` 为 `user:<主设备>` 时拆散用户合并组，否则按自动合并卡 ID 处理。
#[tauri::command]
pub fn separate_merge(card_id: String, shared: State<'_, Arc<HostState>>) -> Result<(), String> {
    if card_id.is_empty() {
        return Err("没有指定要分离的卡片".to_string());
    }
    let auto = card_id.strip_prefix(CARD_PREFIX).is_none();
    if auto {
        let exists = {
            let engine = lock(&shared.engine);
            engine
                .records()
                .iter()
                .any(|r| r.model_key.as_deref() == Some(card_id.as_str()))
        };
        if !exists {
            return Err("未找到该合并卡片".to_string());
        }
        commit_try(&shared, |devices| {
            if devices.separate_auto(&card_id) {
                Ok(())
            } else {
                Err("该卡片已经被分离".to_string())
            }
        })?;
    } else {
        let primary = card_id[CARD_PREFIX.len()..].to_string();
        commit_try(&shared, |devices| {
            if devices.separate(&primary) {
                Ok(())
            } else {
                Err("该设备没有合并记录".to_string())
            }
        })?;
    }
    notify_wake(&shared);
    Ok(())
}

/// 改名：目标是 `user:` 卡片时改合并名，否则改设备别名；空名称恢复原名。
#[tauri::command]
pub fn rename_device(
    target: String,
    name: String,
    shared: State<'_, Arc<HostState>>,
) -> Result<(), String> {
    if let Some(primary) = target.strip_prefix(CARD_PREFIX) {
        let found = {
            let settings = lock(&shared.settings);
            settings
                .devices
                .merges
                .iter()
                .any(|group| group.primary == primary)
        };
        if !found {
            return Err("该合并记录不存在".to_string());
        }
        let title = name.trim();
        if title.is_empty() {
            return Err("合并名称不能为空".to_string());
        }
        let primary = primary.to_string();
        let title = title.to_string();
        commit_try(&shared, |devices| {
            if let Some(group) = devices
                .merges
                .iter_mut()
                .find(|group| group.primary == primary)
            {
                group.name = title;
                Ok(())
            } else {
                Err("该合并记录不存在".to_string())
            }
        })?;
    } else {
        {
            let engine = lock(&shared.engine);
            if engine.record(&target).is_none() {
                return Err(format!("未知设备：{target}"));
            }
        }
        commit(&shared, |devices| devices.rename(&target, &name))?;
    }
    notify_wake(&shared);
    Ok(())
}

/// 读-改-写设置里的设备段：先落盘再同步状态机，任一步失败都不留半成品。
fn commit(
    shared: &HostState,
    mutate: impl FnOnce(&mut voltnest_storage::DeviceSettings),
) -> Result<(), String> {
    commit_try(shared, |devices| {
        mutate(devices);
        Ok(())
    })
}

/// 可失败的版本：校验不通过时不落盘、不同步状态机。
fn commit_try(
    shared: &HostState,
    mutate: impl FnOnce(&mut voltnest_storage::DeviceSettings) -> Result<(), String>,
) -> Result<(), String> {
    crate::commands::write_settings_try(shared, |settings| {
        let result = mutate(&mut settings.devices);
        settings.devices.normalize();
        result
    })
    .map(|_| ())
}

fn notify_wake(shared: &HostState) {
    let _ = shared.tx.send(HostEvent::Wake);
}

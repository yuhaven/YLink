//! Phone file browser and bidirectional ADB transfers.
//!
//! Every adb call redirects stdout/stderr to temp files instead of pipes: adb
//! forks a long-lived server that inherits stdio handles, so a pipe would stay
//! open after the child exits. Progress for `adb push`/`pull` is derived from
//! the destination file size (polled) rather than from adb's output, which is
//! silent until the transfer finishes.
//!
//! Transfers run on a worker thread and publish `transfer.state` notifications;
//! the plugin's stdin loop stays responsive, so the UI can keep polling and the
//! user can see progress without freezing.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::adb;

const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const POLL: Duration = Duration::from_millis(250);
const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// `2026-11-16 19:19` from toybox `ls -la`.
    pub modified: String,
    /// `folder` / `image` / `video` / `doc` / `audio` / `archive` / `file`.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FileEntry>,
    pub restricted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferView {
    pub id: String,
    pub direction: String,
    pub name: String,
    pub state: String,
    pub bytes: u64,
    pub total: u64,
    pub percent: f64,
    pub speed: f64,
    pub local_path: Option<String>,
    pub remote_path: Option<String>,
    pub error: Option<String>,
    pub hint: Option<String>,
    pub started_at: u128,
    pub finished_at: Option<u128>,
    pub files_done: usize,
    pub files_total: usize,
}

#[derive(Debug, Clone)]
pub struct FileError {
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
}

impl FileError {
    fn new(code: &'static str, message: impl Into<String>, hint: Option<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint,
        }
    }
}

static EMIT: OnceLock<fn(&str, Value)> = OnceLock::new();
static TRANSFERS: OnceLock<Mutex<HashMap<String, TransferView>>> = OnceLock::new();
static ACTIVE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

pub fn init_emitter(emitter: fn(&str, Value)) {
    let _ = EMIT.set(emitter);
}

fn emit(view: &TransferView) {
    if let Some(emitter) = EMIT.get() {
        if let Ok(value) = serde_json::to_value(view) {
            emitter("transfer.state", value);
        }
    }
}

fn transfers() -> &'static Mutex<HashMap<String, TransferView>> {
    TRANSFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn active() -> &'static Mutex<Option<String>> {
    ACTIVE.get_or_init(|| Mutex::new(None))
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn register(view: TransferView) -> Result<(), FileError> {
    let mut slot = active().lock().unwrap();
    if let Some(current) = slot.as_ref() {
        let running = transfers()
            .lock()
            .unwrap()
            .get(current)
            .map(|t| t.state == "running")
            .unwrap_or(false);
        if running {
            return Err(FileError::new(
                "E_BUSY",
                "已有文件正在传输，请等它完成",
                Some("完成或失败后可再次发送".to_string()),
            ));
        }
    }
    *slot = Some(view.id.clone());
    drop(slot);
    transfers()
        .lock()
        .unwrap()
        .insert(view.id.clone(), view.clone());
    emit(&view);
    Ok(())
}

fn update<F: FnOnce(&mut TransferView)>(id: &str, change: F) {
    let snapshot = {
        let mut map = transfers().lock().unwrap();
        let Some(view) = map.get_mut(id) else { return };
        change(view);
        view.clone()
    };
    emit(&snapshot);
}

fn finish(id: &str, state: &str, error: Option<String>, hint: Option<String>) {
    update(id, |view| {
        view.state = state.to_string();
        view.error = error;
        view.hint = hint;
        view.finished_at = Some(now_millis());
        if state == "done" {
            view.percent = 100.0;
            view.bytes = view.total.max(view.bytes);
        }
    });
    let mut slot = active().lock().unwrap();
    if slot.as_deref() == Some(id) {
        *slot = None;
    }
}

pub fn status(id: &str) -> Option<TransferView> {
    transfers().lock().unwrap().get(id).cloned()
}

pub fn current() -> Option<TransferView> {
    let id = active().lock().unwrap().clone()?;
    status(&id)
}

fn temp_path(tag: &str) -> PathBuf {
    let stamp = now_millis();
    std::env::temp_dir().join(format!(
        "castpane-file-{}-{stamp}-{tag}.txt",
        std::process::id()
    ))
}

fn spawn_redirected(adb: &Path, args: &[String]) -> Result<(Child, PathBuf, PathBuf), String> {
    let out_path = temp_path("out");
    let err_path = temp_path("err");
    let out = std::fs::File::create(&out_path).map_err(|e| format!("创建临时文件失败：{e}"))?;
    let err = std::fs::File::create(&err_path).map_err(|e| format!("创建临时文件失败：{e}"))?;
    let mut command = Command::new(adb);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let child = command
        .spawn()
        .map_err(|e| format!("无法启动 adb：{e}"))?;
    Ok((child, out_path, err_path))
}

fn read_logs(out: &Path, err: &Path) -> (String, String) {
    (
        std::fs::read_to_string(out).unwrap_or_default(),
        std::fs::read_to_string(err).unwrap_or_default(),
    )
}

fn cleanup(out: &Path, err: &Path) {
    let _ = std::fs::remove_file(out);
    let _ = std::fs::remove_file(err);
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Only the shared-storage roots are exposed. Reject `..` and other roots so a
/// bug in the UI can never ask adb to read `/data` or `/system`.
pub fn normalize_remote(path: &str) -> Result<String, FileError> {
    let path = path.trim();
    let path = if path.is_empty() { "/sdcard" } else { path };
    if !(path == "/sdcard"
        || path.starts_with("/sdcard/")
        || path == "/storage/emulated/0"
        || path.starts_with("/storage/emulated/0/"))
    {
        return Err(FileError::new(
            "E_PATH",
            format!("只允许浏览共享存储：{path}"),
            Some("请从手机 /sdcard 开始浏览".to_string()),
        ));
    }
    if path.split('/').any(|part| part == "..") {
        return Err(FileError::new(
            "E_PATH",
            "路径里不能有 ..",
            Some("请从面包屑返回上一级".to_string()),
        ));
    }
    let mut cleaned = String::with_capacity(path.len());
    for part in path.split('/') {
        if part.is_empty() {
            continue;
        }
        cleaned.push('/');
        cleaned.push_str(part);
    }
    if cleaned.is_empty() {
        cleaned.push('/');
    }
    Ok(cleaned)
}

pub fn parent_of(path: &str) -> Option<String> {
    let path = normalize_remote(path).ok()?;
    if path == "/sdcard" || path == "/storage/emulated/0" {
        return None;
    }
    let (parent, _) = path.rsplit_once('/')?;
    if parent.is_empty() || parent == "/storage/emulated" {
        Some("/sdcard".to_string())
    } else if parent == "/storage" {
        Some("/sdcard".to_string())
    } else {
        Some(parent.to_string())
    }
}

pub fn is_restricted(path: &str) -> bool {
    let path = path.trim_end_matches('/');
    path.starts_with("/sdcard/Android/data")
        || path.starts_with("/sdcard/Android/obb")
        || path.starts_with("/storage/emulated/0/Android/data")
        || path.starts_with("/storage/emulated/0/Android/obb")
}

fn kind_of(name: &str, is_dir: bool) -> String {
    if is_dir {
        return "folder".into();
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "heif" => "image",
        "mp4" | "mov" | "mkv" | "avi" | "3gp" | "webm" | "m4v" => "video",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "csv"
        | "json" | "xml" => "doc",
        "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" => "audio",
        "zip" | "rar" | "7z" | "tar" | "gz" | "apk" => "archive",
        _ => "file",
    }
    .to_string()
}

/// Parse toybox `ls -la` output.
///
/// ```text
/// drwxrws--- 3 u0_a170 media_rw    3440 2024-07-24 09:45 BaiduNetdisk
/// -rw-rw---- 1 u0_a170 media_rw 141252 2025-03-17 12:35 1_于瀚文_123522.pdf
/// lrwxrwxrwx 1 root    root          8 2026-11-16 19:00 link -> /sdcard
/// ```
pub fn parse_ls(output: &str, base: &str) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    for line in output.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with("total ") {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 {
            continue;
        }
        let perms = fields[0];
        let is_dir = perms.starts_with('d');
        let is_link = perms.starts_with('l');
        if !is_dir && !perms.starts_with('-') && !is_link {
            continue;
        }
        let name = fields[7..].join(" ");
        if name == "." || name == ".." || name.is_empty() {
            continue;
        }
        let name = name
            .split_once(" -> ")
            .map(|(name, _)| name)
            .unwrap_or(&name)
            .to_string();
        // Hidden files (`.thumbcache`, `.ids`, ...) are noise for a phone
        // file picker; keep the list focused on user files.
        if name.starts_with('.') {
            continue;
        }
        let size = fields[4].parse::<u64>().unwrap_or(0);
        let modified = format!("{} {}", fields[5], fields[6]);
        let path = if base.ends_with('/') {
            format!("{base}{name}")
        } else {
            format!("{base}/{name}")
        };
        entries.push(FileEntry {
            name: name.clone(),
            path,
            is_dir,
            size,
            modified,
            kind: kind_of(&name, is_dir),
        });
        if entries.len() >= MAX_ENTRIES {
            break;
        }
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

pub fn list(adb_path: &Path, serial: &str, path: &str) -> Result<DirListing, FileError> {
    let path = normalize_remote(path)?;
    if is_restricted(&path) {
        return Err(FileError::new(
            "E_PERMISSION",
            "系统限制此目录，无法浏览",
            Some("请换到 Download、DCIM、Pictures 等共享目录".to_string()),
        ));
    }
    // Trailing slash matters: toybox `ls -la /sdcard` prints the symlink
    // itself (`/sdcard -> /storage/self/primary`) instead of its contents.
    let listing_path = format!("{}/", path.trim_end_matches('/'));
    let command = format!("ls -la {}", shell_quote(&listing_path));
    let output = adb::run(
        adb_path,
        &["-s", serial, "shell", &command],
        adb::ADB_TIMEOUT,
    )
    .map_err(|e| FileError::new("E_ADB", format!("读取目录失败：{e}"), None))?;
    if output.timed_out {
        return Err(FileError::new(
            "E_TIMEOUT",
            "手机没有响应（adb 超时）",
            Some("解锁手机、确认同一 Wi-Fi 后重试".to_string()),
        ));
    }
    let combined = output.combined();
    if !output.success && combined.contains("Permission denied") {
        return Err(FileError::new(
            "E_PERMISSION",
            "没有权限读取这个目录",
            Some("请换到 Download、DCIM、Pictures 等共享目录".to_string()),
        ));
    }
    if !output.success && combined.trim().is_empty() {
        return Err(FileError::new(
            "E_ADB",
            "adb 返回失败，但没有任何输出",
            Some("回首页点「重新检测」，或检查手机是否解锁".to_string()),
        ));
    }
    let entries = parse_ls(&output.stdout, &path);
    if entries.is_empty() && !output.success {
        return Err(FileError::new(
            "E_ADB",
            combined.trim().to_string(),
            Some("确认目录存在；必要时点刷新".to_string()),
        ));
    }
    let restricted = is_restricted(&path);
    Ok(DirListing {
        parent: parent_of(&path),
        path,
        entries,
        restricted,
    })
}

#[derive(Debug, Clone)]
struct RemoteMeta {
    size: u64,
    is_dir: bool,
}

fn remote_meta(adb_path: &Path, serial: &str, path: &str) -> Result<RemoteMeta, FileError> {
    let command = format!("stat -c '%s|%F' {}", shell_quote(path));
    let output = adb::run(
        adb_path,
        &["-s", serial, "shell", &command],
        adb::ADB_TIMEOUT,
    )
    .map_err(|e| FileError::new("E_ADB", format!("读取文件信息失败：{e}"), None))?;
    let text = output.stdout.trim();
    if !output.success || text.is_empty() {
        let detail = output.combined();
        let hint = if detail.contains("Permission denied") {
            "这个文件/目录受系统限制，换到 Download 或 DCIM".to_string()
        } else {
            "确认文件仍存在；必要时刷新目录".to_string()
        };
        return Err(FileError::new(
            "E_NOT_FOUND",
            if detail.trim().is_empty() {
                format!("手机上没有找到 {path}")
            } else {
                detail.trim().to_string()
            },
            Some(hint),
        ));
    }
    let (size, kind) = text.split_once('|').unwrap_or((text, ""));
    Ok(RemoteMeta {
        size: size.trim().parse().unwrap_or(0),
        is_dir: kind.trim().contains("directory"),
    })
}

pub fn default_download_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| {
            let drive = std::env::var_os("HOMEDRIVE")?;
            let path = std::env::var_os("HOMEPATH")?;
            Some(PathBuf::from(drive).join(path))
        })
        .unwrap_or_else(std::env::temp_dir);
    home.join("Downloads").join("CastPane")
}

fn unique_local_path(dir: &Path, name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();
    let safe = if safe.trim().is_empty() {
        "file".to_string()
    } else {
        safe
    };
    let candidate = dir.join(&safe);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match safe.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (safe.clone(), String::new()),
    };
    for index in 1..1000 {
        let candidate = dir.join(format!("{stem} ({index}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}", now_millis()))
}

pub fn start_pull(
    adb_path: PathBuf,
    serial: String,
    paths: Vec<String>,
    local_dir: Option<String>,
) -> Result<String, FileError> {
    if paths.is_empty() {
        return Err(FileError::new(
            "E_NO_SELECTION",
            "先选择一个文件或文件夹",
            Some("点文件行右侧的勾选框".to_string()),
        ));
    }
    let local_dir = local_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_download_dir);
    std::fs::create_dir_all(&local_dir).map_err(|e| {
        FileError::new(
            "E_LOCAL",
            format!("无法创建下载目录：{e}"),
            Some(format!("检查 {} 是否可写", local_dir.display())),
        )
    })?;

    let mut items = Vec::new();
    let mut total = 0u64;
    for path in paths {
        let path = normalize_remote(&path)?;
        let meta = remote_meta(&adb_path, &serial, &path)?;
        if !meta.is_dir {
            total = total.saturating_add(meta.size);
        }
        items.push((path, meta));
    }

    let id = format!("pull-{}", now_millis());
    let name = items
        .first()
        .map(|(path, _)| path.rsplit('/').next().unwrap_or(path).to_string())
        .unwrap_or_else(|| "文件".into());
    let view = TransferView {
        id: id.clone(),
        direction: "pull".into(),
        name,
        state: "running".into(),
        bytes: 0,
        total,
        percent: 0.0,
        speed: 0.0,
        local_path: Some(local_dir.display().to_string()),
        remote_path: items.first().map(|(path, _)| path.clone()),
        error: None,
        hint: None,
        started_at: now_millis(),
        finished_at: None,
        files_done: 0,
        files_total: items.len(),
    };
    register(view)?;

    let thread_id = id.clone();
    std::thread::spawn(move || run_pull(thread_id, adb_path, serial, items, local_dir));
    Ok(id)
}

fn run_pull(
    id: String,
    adb_path: PathBuf,
    serial: String,
    items: Vec<(String, RemoteMeta)>,
    local_dir: PathBuf,
) {
    let started = Instant::now();
    let mut completed = 0u64;
    let mut last_emit = Instant::now();
    let mut last_bytes = 0u64;

    let result: Result<(), FileError> = (|| {
        for (index, (remote, meta)) in items.iter().enumerate() {
            let name = remote.rsplit('/').next().unwrap_or(remote);
            let local = unique_local_path(&local_dir, name);
            update(&id, |view| {
                view.name = name.to_string();
                view.remote_path = Some(remote.clone());
                view.local_path = Some(local.display().to_string());
            });

            if let Some(parent) = local.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            let args = vec![
                "-s".to_string(),
                serial.clone(),
                "pull".to_string(),
                remote.clone(),
                local.display().to_string(),
            ];
            let (mut child, out_path, err_path) =
                spawn_redirected(&adb_path, &args).map_err(|e| FileError::new("E_ADB", e, None))?;

            let mut cancelled = false;
            loop {
                std::thread::sleep(POLL);
                if started.elapsed() > TRANSFER_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    cancelled = true;
                    break;
                }
                let exited = child.try_wait().ok().flatten();
                let current_bytes = if meta.is_dir {
                    dir_size(&local)
                } else {
                    std::fs::metadata(&local)
                        .map(|m| m.len())
                        .unwrap_or(0)
                        .min(meta.size)
                };
                let bytes = completed.saturating_add(current_bytes);
                let elapsed = last_emit.elapsed().as_secs_f64().max(0.001);
                let speed = if current_bytes >= last_bytes {
                    (current_bytes - last_bytes) as f64 / elapsed
                } else {
                    0.0
                };
                if last_emit.elapsed() >= Duration::from_millis(200) {
                    last_emit = Instant::now();
                    last_bytes = current_bytes;
                    let total = items
                        .iter()
                        .filter(|(_, m)| !m.is_dir)
                        .map(|(_, m)| m.size)
                        .sum::<u64>();
                    update(&id, |view| {
                        view.bytes = bytes;
                        view.total = total;
                        view.percent = if total == 0 {
                            0.0
                        } else {
                            (bytes as f64 / total as f64 * 100.0).min(99.0)
                        };
                        view.speed = speed;
                        view.files_done = index;
                    });
                }
                if let Some(status) = exited {
                    let (out, err) = read_logs(&out_path, &err_path);
                    cleanup(&out_path, &err_path);
                    if !status.success() {
                        let detail = first_error(&err, &out);
                        return Err(map_adb_error(detail, remote));
                    }
                    if !meta.is_dir && !local.exists() {
                        return Err(FileError::new(
                            "E_LOCAL",
                            format!("下载失败：电脑上没有生成 {name}"),
                            Some("检查下载目录是否可写".to_string()),
                        ));
                    }
                    let local_size = std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0);
                    if !meta.is_dir && local_size != meta.size {
                        return Err(FileError::new(
                            "E_VERIFY",
                            format!("大小不一致：手机 {} 字节，电脑 {} 字节", meta.size, local_size),
                            Some("重试一次；反复失败请检查手机存储".to_string()),
                        ));
                    }
                    break;
                }
            }
            if cancelled {
                return Err(FileError::new(
                    "E_TIMEOUT",
                    "传输超过 30 分钟，已停止",
                    Some("把大文件拆开，或检查 Wi-Fi".to_string()),
                ));
            }
            completed = completed.saturating_add(meta.size);
            update(&id, |view| {
                view.files_done = index + 1;
                view.bytes = completed;
            });
        }
        Ok(())
    })();

    match result {
        Ok(()) => finish(&id, "done", None, None),
        Err(err) => finish(&id, "error", Some(err.message), err.hint),
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            total += dir_size(&entry.path());
        }
    }
    total
}

fn first_error(err: &str, out: &str) -> String {
    let text = if !err.trim().is_empty() { err } else { out };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .last()
        .unwrap_or("adb 没有返回原因")
        .to_string()
}

fn map_adb_error(detail: String, remote: &str) -> FileError {
    if detail.contains("Permission denied") {
        FileError::new(
            "E_PERMISSION",
            "系统拒绝访问这个文件/目录",
            Some("换到 Download、DCIM、Pictures 等共享目录".to_string()),
        )
    } else if detail.contains("No space left") || detail.contains("ENOSPC") {
        FileError::new(
            "E_NO_SPACE",
            "手机存储空间不足",
            Some("清理手机空间后重试".to_string()),
        )
    } else if detail.contains("not found") || detail.contains("does not exist") {
        FileError::new(
            "E_NOT_FOUND",
            format!("手机上没有找到 {remote}"),
            Some("刷新目录后重试".to_string()),
        )
    } else if detail.to_ascii_lowercase().contains("offline")
        || detail.to_ascii_lowercase().contains("device")
    {
        FileError::new(
            "E_OFFLINE",
            "手机已离线",
            Some("确认手机与电脑在同一 Wi-Fi，回首页重新连接".to_string()),
        )
    } else {
        FileError::new("E_ADB", format!("adb 失败：{detail}"), Some("重试；必要时回首页重新检测手机".to_string()))
    }
}

pub fn start_push(
    adb_path: PathBuf,
    serial: String,
    local_paths: Vec<String>,
    remote_dir: String,
) -> Result<String, FileError> {
    if local_paths.is_empty() {
        return Err(FileError::new(
            "E_NO_SELECTION",
            "先选择要发送的文件或文件夹",
            Some("点「发送到手机」，或把文件拖进窗口".to_string()),
        ));
    }
    let remote_dir = normalize_remote(&remote_dir)?;
    let mut items = Vec::new();
    let mut total = 0u64;
    for local in local_paths {
        let path = PathBuf::from(&local);
        if !path.exists() {
            return Err(FileError::new(
                "E_NOT_FOUND",
                format!("电脑上找不到 {local}"),
                Some("重新选择文件".to_string()),
            ));
        }
        let is_dir = path.is_dir();
        let size = if is_dir { 0 } else { path.metadata().map(|m| m.len()).unwrap_or(0) };
        total = total.saturating_add(size);
        items.push((path, is_dir, size));
    }

    let id = format!("push-{}", now_millis());
    let name = items
        .first()
        .and_then(|(path, _, _)| path.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "文件".into());
    let view = TransferView {
        id: id.clone(),
        direction: "push".into(),
        name,
        state: "running".into(),
        bytes: 0,
        total,
        percent: 0.0,
        speed: 0.0,
        local_path: items
            .first()
            .map(|(path, _, _)| path.display().to_string()),
        remote_path: Some(remote_dir.clone()),
        error: None,
        hint: None,
        started_at: now_millis(),
        finished_at: None,
        files_done: 0,
        files_total: items.len(),
    };
    register(view)?;

    let id_clone = id.clone();
    std::thread::spawn(move || run_push(id_clone, adb_path, serial, items, remote_dir));
    Ok(id)
}

fn run_push(
    id: String,
    adb_path: PathBuf,
    serial: String,
    items: Vec<(PathBuf, bool, u64)>,
    remote_dir: String,
) {
    let started = Instant::now();
    let mut completed = 0u64;
    let mut last_emit = Instant::now();
    let mut last_bytes = 0u64;

    let result: Result<(), FileError> = (|| {
        let mkdir = format!("mkdir -p {}", shell_quote(&remote_dir));
        let output = adb::run(
            &adb_path,
            &["-s", &serial, "shell", &mkdir],
            adb::ADB_TIMEOUT,
        )
        .map_err(|e| FileError::new("E_ADB", format!("创建手机目录失败：{e}"), None))?;
        if !output.success {
            return Err(map_adb_error(output.combined(), &remote_dir));
        }

        for (index, (local, is_dir, size)) in items.iter().enumerate() {
            let name = local
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "file".into());
            let remote = format!("{remote_dir}/{name}");
            update(&id, |view| {
                view.name = name.clone();
                view.remote_path = Some(remote.clone());
            });

            let args = vec![
                "-s".to_string(),
                serial.clone(),
                "push".to_string(),
                local.display().to_string(),
                remote_dir.clone(),
            ];
            let (mut child, out_path, err_path) =
                spawn_redirected(&adb_path, &args).map_err(|e| FileError::new("E_ADB", e, None))?;

            loop {
                std::thread::sleep(POLL);
                if started.elapsed() > TRANSFER_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    cleanup(&out_path, &err_path);
                    return Err(FileError::new(
                        "E_TIMEOUT",
                        "传输超过 30 分钟，已停止",
                        Some("把大文件拆开，或检查 Wi-Fi".to_string()),
                    ));
                }
                let exited = child.try_wait().ok().flatten();
                let current_bytes = if *is_dir {
                    0
                } else {
                    remote_meta(&adb_path, &serial, &remote)
                        .map(|m| m.size)
                        .unwrap_or(0)
                };
                let bytes = completed + current_bytes.min(*size);
                let elapsed = last_emit.elapsed().as_secs_f64().max(0.001);
                let speed = if current_bytes >= last_bytes {
                    (current_bytes - last_bytes) as f64 / elapsed
                } else {
                    0.0
                };
                if last_emit.elapsed() >= Duration::from_millis(200) {
                    last_emit = Instant::now();
                    last_bytes = current_bytes;
                    let total = items.iter().map(|(_, _, size)| *size).sum::<u64>();
                    update(&id, |view| {
                        view.bytes = bytes;
                        view.total = total;
                        view.percent = if total == 0 {
                            0.0
                        } else {
                            (bytes as f64 / total as f64 * 100.0).min(99.0)
                        };
                        view.speed = speed;
                        view.files_done = index;
                    });
                }
                if let Some(status) = exited {
                    let (out, err) = read_logs(&out_path, &err_path);
                    cleanup(&out_path, &err_path);
                    if !status.success() {
                        let detail = first_error(&err, &out);
                        return Err(map_adb_error(detail, &remote));
                    }
                    if !*is_dir {
                        let remote_size = remote_meta(&adb_path, &serial, &remote)
                            .map(|m| m.size)
                            .unwrap_or(0);
                        if remote_size != *size {
                            return Err(FileError::new(
                                "E_VERIFY",
                                format!(
                                    "大小不一致：电脑 {size} 字节，手机 {remote_size} 字节"
                                ),
                                Some("重试；反复失败请检查手机存储".to_string()),
                            ));
                        }
                    }
                    break;
                }
            }
            completed = completed.saturating_add(*size);
            update(&id, |view| {
                view.files_done = index + 1;
                view.bytes = completed;
            });
        }
        Ok(())
    })();

    match result {
        Ok(()) => finish(&id, "done", None, None),
        Err(err) => finish(&id, "error", Some(err.message), err.hint),
    }
}

pub fn open_folder(path: &str) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err(format!("目录不存在：{}", path.display()));
    }
    let mut command = Command::new("explorer.exe");
    command.arg(path.as_os_str());
    command
        .spawn()
        .map_err(|e| format!("无法打开资源管理器：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
total 4106
drwxrws--- 3 u0_a170 media_rw    3440 2024-07-24 09:45 .hidden
drwxrws--- 3 u0_a170 media_rw    3440 2024-07-24 09:45 BaiduNetdisk
-rw-rw---- 1 u0_a170 media_rw 141252 2025-03-17 12:35 1_于瀚文_123522.pdf
-rw-rw---- 1 u0_a170 media_rw  19934 2021-12-24 23:14 photo one.PNG
lrwxrwxrwx 1 root    root          8 2026-11-16 19:00 link -> /sdcard
drwxrws--- 2 u0_a170 media_rw    3440 2026-08-15 17:55 Browser
";

    #[test]
    fn parses_toybox_ls() {
        let entries = parse_ls(SAMPLE, "/sdcard/Download");
        assert_eq!(entries.len(), 5, "hidden files are skipped: {entries:?}");
        assert_eq!(entries[0].name, "BaiduNetdisk");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].path, "/sdcard/Download/BaiduNetdisk");
        let pdf = entries.iter().find(|e| e.name.ends_with(".pdf")).unwrap();
        assert_eq!(pdf.size, 141252);
        assert_eq!(pdf.kind, "doc");
        assert_eq!(pdf.modified, "2025-03-17 12:35");
        let png = entries.iter().find(|e| e.name == "photo one.PNG").unwrap();
        assert_eq!(png.kind, "image");
        assert_eq!(png.size, 19934);
        let link = entries.iter().find(|e| e.name == "link").unwrap();
        assert!(!link.is_dir);
    }

    #[test]
    fn sorts_folders_first() {
        let entries = parse_ls(SAMPLE, "/sdcard");
        assert!(entries[0].is_dir);
        assert!(entries[1].is_dir);
        assert!(!entries[2].is_dir);
    }

    #[test]
    fn normalizes_and_rejects_paths() {
        assert_eq!(normalize_remote("/sdcard/").unwrap(), "/sdcard");
        assert_eq!(
            normalize_remote("/sdcard/Download//x").unwrap(),
            "/sdcard/Download/x"
        );
        assert_eq!(
            normalize_remote("/storage/emulated/0/DCIM").unwrap(),
            "/storage/emulated/0/DCIM"
        );
        assert!(normalize_remote("/data/local/tmp").is_err());
        assert!(normalize_remote("/sdcard/../data").is_err());
    }

    #[test]
    fn parent_walks_back_to_sdcard() {
        assert_eq!(parent_of("/sdcard/Download").as_deref(), Some("/sdcard"));
        assert_eq!(parent_of("/sdcard").as_deref(), None);
        assert_eq!(
            parent_of("/storage/emulated/0/DCIM/Camera").as_deref(),
            Some("/storage/emulated/0/DCIM")
        );
    }

    #[test]
    fn restricts_android_data() {
        assert!(is_restricted("/sdcard/Android/data"));
        assert!(is_restricted("/sdcard/Android/data/com.foo"));
        assert!(!is_restricted("/sdcard/Download"));
    }

    #[test]
    fn unique_names_do_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("castpane-unique-{}", now_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        let next = unique_local_path(&dir, "a.txt");
        assert_eq!(next.file_name().unwrap(), "a (1).txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn classifies_common_files() {
        assert_eq!(kind_of("x.jpg", false), "image");
        assert_eq!(kind_of("x.MP4", false), "video");
        assert_eq!(kind_of("x.pdf", false), "doc");
        assert_eq!(kind_of("x.mp3", false), "audio");
        assert_eq!(kind_of("x.zip", false), "archive");
        assert_eq!(kind_of("x.bin", false), "file");
        assert_eq!(kind_of("folder", true), "folder");
    }
}

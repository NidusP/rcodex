//! 将请求中的会话声明与 Codex 原生历史关联；只读元数据，不读会话正文。
//!
//! 本地探针已核实 codex-cli 0.153.4 在两个并发会话及恢复请求中发送
//! `session-id` / `thread-id`，且值等于 rollout 首行 `session_meta.payload.id`。
//! `x-codex-turn-metadata` 还提供每轮的 `turn_id`。其他版本仍需文件核验，
//! 不能仅凭一个看似合理的 Header 就把请求归到某个会话。

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_METADATA_HEADER: usize = 16 * 1024;
const MAX_META_LINE: usize = 256 * 1024;
const MAX_SCAN_ENTRIES: usize = 20_000;
const MAX_DIRECTORY_DEPTH: usize = 8;
const MAX_MATCHING_FILES: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityCandidates {
    pub native_thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub sources: Vec<String>,
    #[serde(default)]
    pub ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationIdentity {
    pub native_thread_id: Option<String>,
    pub turn_id: Option<String>,
    /// verified / pending / ambiguous / unmatched，后端和界面共用这组状态。
    pub status: String,
    /// 相对登记 CODEX_HOME 的路径；不会将请求里的任意路径拿来读取。
    pub rollout_paths: Vec<String>,
    pub sources: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct WireClaims {
    session_id: Option<String>,
    thread_id: Option<String>,
    conversation_id: Option<String>,
    turn_id: Option<String>,
}

#[derive(Default)]
struct Claims {
    threads: BTreeSet<String>,
    turns: BTreeSet<String>,
    sources: BTreeSet<String>,
}

impl Claims {
    fn thread(&mut self, value: Option<&str>, source: &str) {
        if let Some(id) = canonical_id(value) {
            self.threads.insert(id);
            self.sources.insert(source.to_owned());
        }
    }

    fn turn(&mut self, value: Option<&str>, source: &str) {
        if let Some(id) = canonical_id(value) {
            self.turns.insert(id);
            self.sources.insert(source.to_owned());
        }
    }

    fn wire(&mut self, wire: WireClaims, source: &str, allow_conversation_id: bool) {
        self.thread(wire.session_id.as_deref(), &format!("{source}.session_id"));
        self.thread(wire.thread_id.as_deref(), &format!("{source}.thread_id"));
        if allow_conversation_id {
            self.thread(
                wire.conversation_id.as_deref(),
                &format!("{source}.conversation_id"),
            );
        }
        self.turn(wire.turn_id.as_deref(), &format!("{source}.turn_id"));
    }
}

/// 提取声明不等于验证。所有来源必须一致，再由工作线程核对原生元数据。
/// x-client-request-id 即使在某个版本恰好等于会话 ID，也不作为身份来源。
pub fn extract_candidates(headers: &HeaderMap, body: &[u8]) -> IdentityCandidates {
    let mut claims = Claims::default();
    for name in ["session-id", "thread-id"] {
        for value in headers.get_all(name) {
            claims.thread(value.to_str().ok(), &format!("header:{name}"));
        }
    }
    for value in headers.get_all("x-codex-turn-metadata") {
        if value.as_bytes().len() <= MAX_METADATA_HEADER {
            if let Ok(wire) = serde_json::from_slice(value.as_bytes()) {
                claims.wire(wire, "header:x-codex-turn-metadata", false);
            }
        }
    }
    // 兼容旧入口的顶层候选字段，但不递归查找工具参数/用户文本。使用小结构
    // 反序列化时未知内容被跳过，不为整段 input 另外建立一棵 JSON 对象树。
    if body.len() <= MAX_BODY_BYTES {
        if let Ok(wire) = serde_json::from_slice(body) {
            claims.wire(wire, "body", true);
        }
    }
    IdentityCandidates {
        native_thread_id: only_value(&claims.threads),
        turn_id: only_value(&claims.turns),
        sources: claims.sources.into_iter().collect(),
        ambiguous: claims.threads.len() > 1 || claims.turns.len() > 1,
    }
}

/// 此函数有文件 I/O，只能在归档工作线程或 spawn_blocking 中调用。
/// 暂时不读原生 SQLite：只核对命中文件名的 rollout 首行，避免猜测跨版本 schema。
pub fn verify_identity(
    home: &Path,
    _sqlite_home: Option<&Path>,
    candidates: &IdentityCandidates,
) -> ConversationIdentity {
    let mut result = ConversationIdentity {
        native_thread_id: canonical_id(candidates.native_thread_id.as_deref()),
        turn_id: canonical_id(candidates.turn_id.as_deref()),
        status: "unmatched".into(),
        rollout_paths: Vec::new(),
        sources: candidates.sources.clone(),
        reason: None,
    };
    if candidates.ambiguous {
        result.status = "ambiguous".into();
        result.reason = Some("conflicting_identity_claims".into());
        return result;
    }
    let Some(thread_id) = result.native_thread_id.clone() else {
        result.reason = Some("native_thread_id_missing".into());
        return result;
    };
    result.status = "pending".into();
    // 根目录别名由控制面登记时规范化；这里也规范化一次以支持 macOS /tmp
    // 这样的系统别名。sessions 以下的任何链接都不能作为候选历史文件。
    let Ok(root) = home.canonicalize() else {
        result.reason = Some("codex_home_unavailable".into());
        return result;
    };
    if !root.is_dir() {
        result.reason = Some("codex_home_unavailable".into());
        return result;
    }
    let mut scan = Scan::default();
    for name in ["sessions", "archived_sessions"] {
        collect_candidates(&root.join(name), &thread_id, 0, &mut scan);
    }
    // 达到扫描边界时不能把“当前只找到一个”当作“只有一个”；留待后续关联。
    if scan.limited {
        result.reason = Some("history_scan_limit_exceeded".into());
        return result;
    }
    let mut paths = BTreeSet::new();
    for path in scan.paths {
        match first_session_id(&root, &path) {
            Ok(Some(id)) if id == thread_id => {
                if let Ok(relative) = path.strip_prefix(&root) {
                    paths.insert(relative.to_string_lossy().replace('\\', "/"));
                }
            }
            Ok(_) => {}
            Err(_) => scan.incomplete = true,
        }
    }
    result.rollout_paths = paths.into_iter().collect();
    if result.rollout_paths.len() > 1 {
        result.status = "ambiguous".into();
        result.reason = Some("multiple_rollout_files_match".into());
    } else if scan.incomplete {
        result.reason = Some("history_metadata_unavailable".into());
    } else if result.rollout_paths.len() == 1 {
        result.status = "verified".into();
    } else {
        result.reason = Some("native_thread_not_found_yet".into());
    }
    result
}

fn canonical_id(value: Option<&str>) -> Option<String> {
    let id = Uuid::parse_str(value?).ok()?;
    (!id.is_nil()).then(|| id.hyphenated().to_string())
}

fn only_value(values: &BTreeSet<String>) -> Option<String> {
    if values.len() == 1 {
        values.iter().next().cloned()
    } else {
        None
    }
}

#[derive(Default)]
struct Scan {
    entries: usize,
    paths: Vec<PathBuf>,
    limited: bool,
    incomplete: bool,
}

fn collect_candidates(root: &Path, thread_id: &str, depth: usize, scan: &mut Scan) {
    if scan.limited {
        return;
    }
    if depth > MAX_DIRECTORY_DEPTH {
        scan.limited = true;
        return;
    }
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            scan.incomplete = true;
            return;
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        scan.incomplete = true;
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        scan.incomplete = true;
        return;
    };
    for entry in entries {
        scan.entries += 1;
        if scan.entries > MAX_SCAN_ENTRIES {
            scan.limited = true;
            return;
        }
        let Ok(entry) = entry else {
            scan.incomplete = true;
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            scan.incomplete = true;
            continue;
        };
        if kind.is_symlink() {
            // 不能跟随链接，即使它当前仍指向根内；避免将链接目标的变化误作历史。
            scan.incomplete = true;
        } else if kind.is_dir() {
            collect_candidates(&entry.path(), thread_id, depth + 1, scan);
        } else if kind.is_file()
            && matching_filename(&entry.file_name().to_string_lossy(), thread_id)
        {
            scan.paths.push(entry.path());
            if scan.paths.len() > MAX_MATCHING_FILES {
                scan.limited = true;
                return;
            }
        }
    }
}

fn matching_filename(name: &str, thread_id: &str) -> bool {
    name.starts_with("rollout-")
        && name
            .strip_suffix(".jsonl")
            .and_then(|name| name.strip_suffix(thread_id))
            .is_some_and(|prefix| prefix.ends_with('-'))
}

fn first_session_id(root: &Path, path: &Path) -> Result<Option<String>, ()> {
    let file = open_beneath(root, path)?;
    // take 在 BufReader 外包住文件：最多从文件读取首行预算+1 字节，不会让
    // read_until 先把一个巨型行加载完再检查，也不会继续读取第二行对话正文。
    let mut reader = BufReader::with_capacity(1, file.take((MAX_META_LINE + 1) as u64));
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line).map_err(|_| ())?;
    if line.len() > MAX_META_LINE || !line.ends_with(b"\n") {
        return Err(());
    }
    #[derive(Deserialize)]
    struct Meta {
        #[serde(rename = "type")]
        kind: String,
        payload: MetaPayload,
    }
    #[derive(Deserialize)]
    struct MetaPayload {
        id: String,
    }
    let record: Meta = serde_json::from_slice(&line).map_err(|_| ())?;
    if record.kind != "session_meta" {
        return Ok(None);
    }
    Ok(canonical_id(Some(&record.payload.id)))
}

#[cfg(unix)]
fn open_beneath(root: &Path, path: &Path) -> Result<File, ()> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    };
    let relative = path.strip_prefix(root).map_err(|_| ())?;
    let components: Vec<_> = relative.components().collect();
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(|_| ())?;
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(());
        };
        let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| ())?;
        let last = index + 1 == components.len();
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if last { 0 } else { libc::O_DIRECTORY };
        // 逐层 openat 保持已打开目录句柄，防止校验后某个父目录被换成越根链接。
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(());
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    if components.is_empty() || !directory.metadata().map_err(|_| ())?.is_file() {
        return Err(());
    }
    Ok(directory)
}

#[cfg(windows)]
fn open_beneath(root: &Path, path: &Path) -> Result<File, ()> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

    let relative = path.strip_prefix(root).map_err(|_| ())?;
    let mut cursor = root.to_path_buf();
    let mut directories = Vec::new();
    let open_directory = |path: &Path| -> Result<File, ()> {
        let handle = OpenOptions::new()
            .read(true)
            // FILE_SHARE_READ: 持有目录期间禁止重命名/删除它，避免父目录换链。
            .share_mode(1)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)
            .map_err(|_| ())?;
        let metadata = handle.metadata().map_err(|_| ())?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(());
        }
        Ok(handle)
    };
    directories.push(open_directory(root)?);
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(());
        };
        cursor.push(name);
        if index + 1 == components.len() {
            let file = OpenOptions::new()
                .read(true)
                // 允许 Codex 继续追加原生历史，但验证时不允许替换这个文件。
                .share_mode(1 | 2)
                .custom_flags(0x0020_0000)
                .open(&cursor)
                .map_err(|_| ())?;
            let metadata = file.metadata().map_err(|_| ())?;
            if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
                return Err(());
            }
            return Ok(file);
        }
        directories.push(open_directory(&cursor)?);
    }
    Err(())
}

#[cfg(not(any(unix, windows)))]
fn open_beneath(_root: &Path, _path: &Path) -> Result<File, ()> {
    Err(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const ONE: &str = "01a0aaec-15ab-7352-b3ea-3cd3a99cc56a";
    const TWO: &str = "01a0aaec-15a7-7503-813a-1c71fa228437";
    const TURN: &str = "01a0aaec-15b3-7a52-a182-bc83f8c9bef8";
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("rcodex-identity-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self(root.canonicalize().unwrap())
        }
        fn write(&self, directory: &str, name_id: &str, meta_id: &str) -> PathBuf {
            let path = self
                .0
                .join(directory)
                .join(format!("rollout-2026-09-16T00-00-00-{name_id}.jsonl"));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{meta_id}\"}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"text\":\"must not read\"}}}}\n")).unwrap();
            path
        }
        fn verify(&self, id: &str) -> ConversationIdentity {
            verify_identity(
                &self.0,
                None,
                &IdentityCandidates {
                    native_thread_id: Some(id.into()),
                    ..Default::default()
                },
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn headers(entries: &[(&'static str, String)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in entries {
            headers.append(*name, value.parse().unwrap());
        }
        headers
    }
    #[test]
    fn probed_headers_include_native_thread_and_turn_without_nested_body_guessing() {
        let headers = headers(&[
            ("session-id", ONE.into()),
            ("thread-id", ONE.into()),
            (
                "x-codex-turn-metadata",
                format!(
                    "{{\"session_id\":\"{ONE}\",\"thread_id\":\"{ONE}\",\"turn_id\":\"{TURN}\"}}"
                ),
            ),
        ]);
        let claims = extract_candidates(
            &headers,
            format!("{{\"metadata\":{{\"thread_id\":\"{TWO}\"}}}}").as_bytes(),
        );
        assert_eq!(claims.native_thread_id.as_deref(), Some(ONE));
        assert_eq!(claims.turn_id.as_deref(), Some(TURN));
        assert!(!claims.ambiguous);
        assert_eq!(claims.sources.len(), 5);
    }
    #[test]
    fn unsupported_guesses_and_request_id_are_not_session_identity() {
        let claims = extract_candidates(
            &headers(&[
                ("x-client-request-id", ONE.into()),
                ("x-codex-thread-id", ONE.into()),
            ]),
            b"{}",
        );
        assert_eq!(claims.native_thread_id, None);
        assert_eq!(
            verify_identity(Path::new("/nonexistent"), None, &claims).status,
            "unmatched"
        );
    }
    #[test]
    fn duplicate_headers_and_cross_source_conflicts_remain_ambiguous() {
        for headers in [
            headers(&[("session-id", ONE.into()), ("session-id", TWO.into())]),
            headers(&[("thread-id", ONE.into())]),
        ] {
            let claims =
                extract_candidates(&headers, format!("{{\"session_id\":\"{TWO}\"}}").as_bytes());
            assert!(claims.ambiguous);
            assert_eq!(
                verify_identity(Path::new("/nonexistent"), None, &claims).status,
                "ambiguous"
            );
        }
    }
    #[test]
    fn concurrent_conversations_and_archived_moves_keep_independent_ids() {
        let f = Fixture::new();
        let first = f.write("sessions/2026/09/16", ONE, ONE);
        f.write("sessions/2026/09/16", TWO, TWO);
        let before = fs::read(&first).unwrap();
        assert_eq!(f.verify(ONE).status, "verified");
        assert_eq!(f.verify(TWO).native_thread_id.as_deref(), Some(TWO));
        assert_eq!(fs::read(&first).unwrap(), before);
        fs::create_dir_all(f.0.join("archived_sessions")).unwrap();
        fs::rename(
            &first,
            f.0.join("archived_sessions")
                .join(first.file_name().unwrap()),
        )
        .unwrap();
        let archived = f.verify(ONE);
        assert_eq!(archived.status, "verified");
        assert!(archived.rollout_paths[0].starts_with("archived_sessions/"));
    }
    #[test]
    fn duplicate_files_are_ambiguous_missing_pending_and_wrong_meta_never_verified() {
        let f = Fixture::new();
        assert_eq!(f.verify(ONE).status, "pending");
        f.write("sessions", ONE, TWO);
        assert_eq!(f.verify(ONE).status, "pending");
        f.write("sessions", ONE, ONE);
        f.write("archived_sessions", ONE, ONE);
        assert_eq!(f.verify(ONE).status, "ambiguous");
        assert_eq!(f.verify(ONE).rollout_paths.len(), 2);
    }
    #[test]
    fn missing_identity_and_traversal_values_are_unmatched() {
        for id in [
            "",
            "../../elsewhere",
            "00000000-0000-0000-0000-000000000000",
        ] {
            assert_eq!(Fixture::new().verify(id).status, "unmatched");
        }
    }
    #[test]
    fn incomplete_large_or_non_metadata_first_record_is_not_read_past() {
        let f = Fixture::new();
        let path = f.write("sessions", ONE, ONE);
        for bytes in [vec![b'x'; MAX_META_LINE + 1], format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{ONE}\"}}}}").into_bytes(), format!("{{\"type\":\"event_msg\",\"payload\":{{\"id\":\"{ONE}\"}}}}\n{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{ONE}\"}}}}\n").into_bytes()] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(f.verify(ONE).status, "pending");
        }
    }
    #[test]
    fn filename_filter_does_not_read_other_conversation_bodies() {
        let f = Fixture::new();
        f.write("sessions", ONE, ONE);
        let other = f.write("sessions", TWO, TWO);
        fs::write(other, b"not even valid json").unwrap();
        assert_eq!(f.verify(ONE).status, "verified");
    }
    #[cfg(unix)]
    #[test]
    fn symlink_file_directory_and_parent_swap_are_rejected() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let outside = Fixture::new();
        let target = outside.write("sessions", ONE, ONE);
        fs::create_dir_all(f.0.join("sessions")).unwrap();
        let link = f.0.join("sessions").join(target.file_name().unwrap());
        symlink(&target, &link).unwrap();
        assert_eq!(f.verify(ONE).status, "pending");
        assert!(open_beneath(&f.0, &link).is_err());
        fs::remove_file(link).unwrap();
        symlink(outside.0.join("sessions"), f.0.join("sessions/child")).unwrap();
        assert!(open_beneath(
            &f.0,
            &f.0.join("sessions/child").join(target.file_name().unwrap())
        )
        .is_err());
        assert_eq!(f.verify(ONE).status, "pending");
    }
    #[test]
    fn scope_isolation_does_not_search_neighboring_homes() {
        let one = Fixture::new();
        let two = Fixture::new();
        one.write("sessions", ONE, ONE);
        assert_eq!(one.verify(ONE).status, "verified");
        assert_eq!(two.verify(ONE).status, "pending");
    }
}

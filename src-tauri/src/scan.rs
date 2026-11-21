//! Exact-duplicate scan pipeline: discover → group by size → SHA-256 → group.
//! Pure Rust, no Tauri types, so it can be tested directly.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use rusqlite::{params, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

const BATCH: usize = 500;
const HASH_BATCH: usize = 100;
const BUF_SIZE: usize = 128 * 1024;

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "tif", "tiff"];
const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "mov", "mkv", "avi", "webm", "wmv", "flv", "mpg", "mpeg"];
const AUDIO_EXTS: &[&str] = &["mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "wma"];
const DOCUMENT_EXTS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "epub",
];
const ARCHIVE_EXTS: &[&str] = &["zip", "rar", "7z", "tar", "gz", "bz2", "xz"];

/// Every extension Sift can scan. Only images get previews, dimensions and similarity
/// analysis; the rest are matched by exact content (SHA-256) like any other file.
fn supported_exts() -> impl Iterator<Item = &'static str> {
    [IMAGE_EXTS, VIDEO_EXTS, AUDIO_EXTS, DOCUMENT_EXTS, ARCHIVE_EXTS].into_iter().flatten().copied()
}

fn is_supported(ext: &str) -> bool {
    supported_exts().any(|e| e == ext)
}

/// "image" | "video" | "audio" | "document" | "archive" | "other"
pub fn category_of(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let ext = ext.as_str();
    [
        ("image", IMAGE_EXTS),
        ("video", VIDEO_EXTS),
        ("audio", AUDIO_EXTS),
        ("document", DOCUMENT_EXTS),
        ("archive", ARCHIVE_EXTS),
    ]
    .into_iter()
    .find(|(_, list)| list.contains(&ext))
    .map_or("other", |(name, _)| name)
}

/// True if the file's extension is one Sift can scan.
pub fn is_supported_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| is_supported(&e.to_ascii_lowercase()))
        .unwrap_or(false)
}

/// True if any folder on the path is one scans never enter (`.git`, caches, trash...).
pub fn in_excluded_dir(path: &Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_lowercase();
        EXCLUDED_DIRS.contains(&name.as_str()) || name.starts_with(".trash-")
    })
}

/// True for formats the `image` crate decodes here (previews, dimensions, perceptual hashes).
pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Directories that are never descended into (compared case-insensitively).
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".cache",
    "__pycache__",
    "$recycle.bin",
    "system volume information",
    ".trash",
    ".thumbnails",
];

#[derive(Clone, Debug, Default, Serialize)]
pub struct Progress {
    /// "discovering" | "hashing" | "fingerprinting" | "finalizing"
    pub phase: &'static str,
    pub files_found: u64,
    /// Work items in the current phase (files to hash, or to fingerprint) and how many are done.
    pub hash_total: u64,
    pub hash_done: u64,
    pub errors: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Summary {
    pub scan_id: i64,
    /// "completed" | "cancelled"
    pub status: &'static str,
    pub files_seen: u64,
    pub files_hashed: u64,
    /// Files in size-collision groups whose cached hash was still valid.
    pub files_cached: u64,
    /// Images whose perceptual hash was computed (or found undecodable) this scan.
    pub files_fingerprinted: u64,
    pub errors: u64,
}

pub fn begin_scan(conn: &Connection) -> Result<i64, String> {
    conn.execute("INSERT INTO scans DEFAULT VALUES", [])
        .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

/// User-adjustable scan preferences (Settings screen).
#[derive(Clone, Debug)]
pub struct Config {
    /// Directories (absolute) that are never descended into.
    pub excluded_paths: Vec<std::path::PathBuf>,
    /// Lowercase extensions to scan, without the dot.
    pub extensions: Vec<String>,
    /// Compute perceptual hashes for similar-image detection.
    pub similar: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            excluded_paths: Vec::new(),
            extensions: supported_exts().map(String::from).collect(),
            similar: true,
        }
    }
}

impl Config {
    /// Reads preferences from the settings table; missing or invalid values fall back to defaults.
    /// Only supported extensions are ever honoured.
    pub fn load(conn: &Connection) -> Result<Config, String> {
        let mut cfg = Config::default();
        if let Some(raw) = crate::db::get_setting(conn, "scan.excluded_paths")? {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&raw) {
                cfg.excluded_paths = list.into_iter().map(Into::into).collect();
            }
        }
        if let Some(raw) = crate::db::get_setting(conn, "scan.file_types")? {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&raw) {
                cfg.extensions = list
                    .into_iter()
                    .map(|e| e.to_ascii_lowercase())
                    .filter(|e| is_supported(e))
                    .collect();
            }
        }
        cfg.similar = crate::similar::enabled(conn)?;
        Ok(cfg)
    }

    fn wants(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| self.extensions.contains(&e.to_ascii_lowercase()))
            .unwrap_or(false)
    }

    pub(crate) fn is_excluded(&self, entry: &DirEntry) -> bool {
        if entry.depth() == 0 || !entry.file_type().is_dir() {
            return false;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        EXCLUDED_DIRS.contains(&name.as_str())
            || name.starts_with(".trash-")
            || self.excluded_paths.iter().any(|p| entry.path().starts_with(p))
    }
}

pub(crate) fn mtime_ns(md: &fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Identifies the physical file so hard links can be recognised (Unix only).
#[cfg(unix)]
pub(crate) fn file_id(md: &fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    Some(format!("{}:{}", md.dev(), md.ino()))
}
#[cfg(not(unix))]
pub(crate) fn file_id(_md: &fs::Metadata) -> Option<String> {
    None
}

struct Row {
    folder_id: i64,
    path: String,
    size: i64,
    mtime_ns: i64,
    file_id: Option<String>,
}

fn flush_rows(conn: &mut Connection, rows: &mut Vec<Row>, scan_id: i64) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    {
        // A changed size or mtime invalidates the cached hash.
        let mut stmt = tx
            .prepare_cached(
                "INSERT INTO files (folder_id, path, size, mtime_ns, file_id, last_seen_scan)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(path) DO UPDATE SET
                    folder_id = excluded.folder_id,
                    file_id = excluded.file_id,
                    last_seen_scan = excluded.last_seen_scan,
                    sha256 = CASE WHEN files.size = excluded.size AND files.mtime_ns = excluded.mtime_ns
                                  THEN files.sha256 ELSE NULL END,
                    width = CASE WHEN files.size = excluded.size AND files.mtime_ns = excluded.mtime_ns
                                 THEN files.width ELSE NULL END,
                    height = CASE WHEN files.size = excluded.size AND files.mtime_ns = excluded.mtime_ns
                                  THEN files.height ELSE NULL END,
                    phash = CASE WHEN files.size = excluded.size AND files.mtime_ns = excluded.mtime_ns
                                 THEN files.phash ELSE NULL END,
                    sharpness = CASE WHEN files.size = excluded.size AND files.mtime_ns = excluded.mtime_ns
                                     THEN files.sharpness ELSE NULL END,
                    phash_failed = CASE WHEN files.size = excluded.size AND files.mtime_ns = excluded.mtime_ns
                                        THEN files.phash_failed ELSE 0 END,
                    size = excluded.size,
                    mtime_ns = excluded.mtime_ns",
            )
            .map_err(|e| e.to_string())?;
        for r in rows.iter() {
            stmt.execute(params![r.folder_id, r.path, r.size, r.mtime_ns, r.file_id, scan_id])
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    rows.clear();
    Ok(())
}

/// Streams the file through SHA-256. Returns `None` if cancelled mid-file.
pub(crate) fn hash_file(path: &str, cancel: &AtomicBool) -> std::io::Result<Option<String>> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF_SIZE];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(Some(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()))
}

fn flush_hashes(conn: &mut Connection, pending: &mut Vec<(i64, String)>) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare_cached("UPDATE files SET sha256 = ?2 WHERE id = ?1")
            .map_err(|e| e.to_string())?;
        for (id, h) in pending.iter() {
            stmt.execute(params![id, h]).map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    pending.clear();
    Ok(())
}

const COLLIDING: &str = "last_seen_scan = ?1 AND size IN (
    SELECT size FROM files WHERE last_seen_scan = ?1 GROUP BY size HAVING COUNT(*) > 1)";

pub fn run(
    conn: &mut Connection,
    scan_id: i64,
    cancel: &AtomicBool,
    config: &Config,
    on_progress: &mut dyn FnMut(&Progress),
) -> Result<Summary, String> {
    let mut p = Progress { phase: "discovering", ..Default::default() };
    let mut scanned_folders: Vec<i64> = Vec::new();
    let mut batch: Vec<Row> = Vec::with_capacity(BATCH);
    let mut cancelled = false;

    // 1. Discover
    'folders: for (folder_id, root) in crate::db::folder_rows(conn)? {
        if !Path::new(&root).is_dir() {
            continue; // missing folder: leave its cached rows untouched
        }
        scanned_folders.push(folder_id);
        let walker = WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !config.is_excluded(e));
        for entry in walker {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break 'folders;
            }
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    p.errors += 1;
                    continue;
                }
            };
            // Symlinks report as symlinks (not files) when links aren't followed.
            if !entry.file_type().is_file() || !config.wants(entry.path()) {
                continue;
            }
            let (Some(path), Ok(md)) = (entry.path().to_str(), entry.metadata()) else {
                p.errors += 1;
                continue;
            };
            if md.len() == 0 {
                continue;
            }
            batch.push(Row {
                folder_id,
                path: path.to_owned(),
                size: md.len() as i64,
                mtime_ns: mtime_ns(&md),
                file_id: file_id(&md),
            });
            p.files_found += 1;
            if batch.len() >= BATCH {
                flush_rows(conn, &mut batch, scan_id)?;
                on_progress(&p);
            }
        }
    }
    flush_rows(conn, &mut batch, scan_id)?;
    on_progress(&p);

    // 2. Hash files that share a size with another file and have no valid cached hash.
    let mut hashed = 0u64;
    let mut cached = 0u64;
    if !cancelled {
        cached = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM files WHERE {COLLIDING} AND sha256 IS NOT NULL"),
                params![scan_id],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| e.to_string())? as u64;

        let candidates: Vec<(i64, String, i64, i64)> = {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT id, path, size, mtime_ns FROM files WHERE {COLLIDING} AND sha256 IS NULL"
                ))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![scan_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
        };

        p.phase = "hashing";
        p.hash_total = candidates.len() as u64;
        on_progress(&p);

        let mut pending: Vec<(i64, String)> = Vec::with_capacity(HASH_BATCH);
        for (id, path, size, mtime) in candidates {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            match hash_file(&path, cancel) {
                Ok(Some(hash)) => {
                    // Discard the hash if the file changed while we were reading it.
                    let unchanged = fs::metadata(&path)
                        .map(|m| m.len() as i64 == size && mtime_ns(&m) == mtime)
                        .unwrap_or(false);
                    if unchanged {
                        pending.push((id, hash));
                        hashed += 1;
                    } else {
                        p.errors += 1;
                    }
                }
                Ok(None) => {
                    cancelled = true;
                    break;
                }
                Err(_) => p.errors += 1,
            }
            p.hash_done += 1;
            if pending.len() >= HASH_BATCH {
                flush_hashes(conn, &mut pending)?;
                on_progress(&p);
            }
        }
        // Persist finished hashes even when cancelled, so the next scan resumes.
        flush_hashes(conn, &mut pending)?;
        on_progress(&p);
    }

    // 3. Fingerprint images for similarity detection (separate from the exact pipeline).
    let mut fingerprinted = 0u64;
    if !cancelled && config.similar {
        let todo = crate::similar::candidates(conn, scan_id)?;
        p.phase = "fingerprinting";
        p.hash_total = todo.len() as u64;
        p.hash_done = 0;
        on_progress(&p);
        if !todo.is_empty() {
            let mut last = 0u64;
            let (n, was_cancelled) = crate::similar::fingerprint(conn, todo, cancel, &mut |done| {
                // The callback is called per file; only report every few to keep events cheap.
                if done - last >= 5 {
                    last = done;
                    p.hash_done = done;
                    on_progress(&p);
                }
            })?;
            fingerprinted = n;
            cancelled = was_cancelled;
        }
    }

    // 4. Finalize. Only a completed scan may sweep rows for files that disappeared.
    p.phase = "finalizing";
    on_progress(&p);
    let status = if cancelled { "cancelled" } else { "completed" };
    if !cancelled {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for fid in &scanned_folders {
            tx.execute(
                "DELETE FROM files WHERE folder_id = ?1 AND last_seen_scan < ?2",
                params![fid, scan_id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE folders SET last_scanned_at = unixepoch() WHERE id = ?1",
                params![fid],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
    }
    conn.execute(
        "UPDATE scans SET finished_at = unixepoch(), status = ?2, files_seen = ?3,
                files_hashed = ?4, files_cached = ?5, errors = ?6 WHERE id = ?1",
        params![scan_id, status, p.files_found as i64, hashed as i64, cached as i64, p.errors as i64],
    )
    .map_err(|e| e.to_string())?;

    Ok(Summary {
        scan_id,
        status,
        files_seen: p.files_found,
        files_hashed: hashed,
        files_cached: cached,
        files_fingerprinted: fingerprinted,
        errors: p.errors,
    })
}

#[derive(Debug, Serialize, PartialEq)]
pub struct DupFile {
    pub id: i64,
    pub path: String,
    pub size: i64,
    /// Identifies the physical file (Unix); used to tell hard links apart. Not sent to the UI.
    #[serde(skip)]
    pub file_id: Option<String>,
    /// Last modified, Unix seconds.
    pub modified: i64,
    /// Creation time, Unix seconds, when the filesystem reports one (filled by `review::enrich`).
    pub created: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Laplacian-variance sharpness (similar groups only); higher is sharper.
    pub sharpness: Option<f64>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct DupGroup {
    /// "exact" | "similar"
    pub kind: &'static str,
    /// Identifies the group: the content hash for exact groups, `similar-<id>` for similar ones.
    pub sha256: String,
    /// Size of each copy for exact groups; of the largest copy for similar ones.
    pub size: i64,
    pub files: Vec<DupFile>,
    /// Bytes recoverable by keeping one physical copy. Hard links don't count.
    pub wasted: i64,
    /// Id of the copy recommended to keep (filled by `review::enrich`).
    pub keep_id: i64,
}

/// Groups of byte-identical files. Hard links to one physical file count as a single copy,
/// and files that have vanished from disk since the last scan are ignored.
pub fn duplicate_groups(conn: &Connection) -> Result<Vec<DupGroup>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, path, size, sha256, file_id, mtime_ns, width, height FROM files
             WHERE sha256 IN (SELECT sha256 FROM files WHERE sha256 IS NOT NULL
                              GROUP BY sha256 HAVING COUNT(*) > 1)
             ORDER BY sha256, path",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<u32>>(6)?,
                r.get::<_, Option<u32>>(7)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut by_hash: HashMap<String, (i64, Vec<(DupFile, Option<String>)>)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for row in rows {
        let (id, path, size, sha, fid, mtime, width, height) = row.map_err(|e| e.to_string())?;
        if !Path::new(&path).is_file() {
            continue;
        }
        let entry = by_hash.entry(sha.clone()).or_insert_with(|| {
            order.push(sha.clone());
            (size, Vec::new())
        });
        entry.1.push((
            DupFile {
                id,
                path,
                size,
                file_id: fid.clone(),
                modified: mtime / 1_000_000_000,
                created: None,
                width,
                height,
                sharpness: None,
            },
            fid,
        ));
    }

    let mut groups = Vec::new();
    for sha in order {
        let (size, files) = by_hash.remove(&sha).unwrap();
        let mut physical: Vec<&str> = Vec::new();
        let mut unknown = 0usize;
        for (_, fid) in &files {
            match fid {
                Some(f) if !physical.contains(&f.as_str()) => physical.push(f),
                Some(_) => {}
                None => unknown += 1,
            }
        }
        let copies = physical.len() + unknown;
        if copies < 2 {
            continue;
        }
        groups.push(DupGroup {
            kind: "exact",
            sha256: sha,
            size,
            wasted: size * (copies as i64 - 1),
            keep_id: 0,
            files: files.into_iter().map(|(f, _)| f).collect(),
        });
    }
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted));
    Ok(groups)
}

#[derive(Debug, Serialize, Default, PartialEq)]
pub struct Stats {
    pub files: i64,
    pub bytes: i64,
    pub groups: i64,
    pub wasted: i64,
    /// Groups of visually similar (not byte-identical) images.
    pub similar_groups: i64,
    /// Bytes permanently freed by cleanup so far.
    pub recovered: i64,
    /// Bytes currently held in quarantine (still on disk until deleted).
    pub quarantined: i64,
    pub last_scan_at: Option<i64>,
}

pub fn stats(conn: &Connection) -> Result<Stats, String> {
    let (files, bytes) = conn
        .query_row("SELECT COUNT(*), COALESCE(SUM(size), 0) FROM files", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(|e| e.to_string())?;
    let last_scan_at = conn
        .query_row(
            "SELECT MAX(finished_at) FROM scans WHERE status = 'completed'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let groups = duplicate_groups(conn)?;
    Ok(Stats {
        files,
        bytes,
        groups: groups.len() as i64,
        wasted: groups.iter().map(|g| g.wasted).sum(),
        similar_groups: crate::similar::group_count(conn)?,
        recovered: crate::cleanup::recovered(conn)?,
        quarantined: crate::cleanup::quarantined_bytes(conn)?,
        last_scan_at,
    })
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Insight {
    pub label: String,
    /// Files in duplicate groups.
    pub count: i64,
    /// Bytes those files could free (every copy but one per group).
    pub wasted: i64,
}

#[derive(Debug, Serialize, Default, PartialEq)]
pub struct Insights {
    pub folders: Vec<Insight>,
    pub types: Vec<Insight>,
}

const TOP_N: usize = 5;

/// Which folders hold the most duplicate files and which file types are duplicated most.
pub fn insights(conn: &Connection) -> Result<Insights, String> {
    let groups = duplicate_groups(conn)?;
    let mut folders: HashMap<String, Insight> = HashMap::new();
    let mut types: HashMap<String, Insight> = HashMap::new();
    for g in &groups {
        let n = g.files.len() as i64;
        // Spread the group's wasted bytes across its files so totals add up.
        let share = g.wasted / n.max(1);
        for f in &g.files {
            let p = Path::new(&f.path);
            let folder = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_uppercase())
                .unwrap_or_else(|| "?".into());
            for (map, key) in [(&mut folders, folder), (&mut types, ext)] {
                let e = map.entry(key.clone()).or_insert(Insight { label: key, count: 0, wasted: 0 });
                e.count += 1;
                e.wasted += share;
            }
        }
    }
    let top = |m: HashMap<String, Insight>| {
        let mut v: Vec<Insight> = m.into_values().collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
        v.truncate(TOP_N);
        v
    };
    Ok(Insights { folders: top(folders), types: top(types) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Env {
        conn: Connection,
        dir: tempfile::TempDir,
    }

    fn env() -> Env {
        let conn = crate::db::init(Connection::open_in_memory().unwrap()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::db::add_folder(&conn, dir.path().to_str().unwrap()).unwrap();
        Env { conn, dir }
    }

    fn write(env: &Env, name: &str, content: &[u8]) -> PathBuf {
        let p = env.dir.path().join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }

    fn scan(env: &mut Env) -> Summary {
        scan_with(env, &AtomicBool::new(false))
    }

    fn scan_with(env: &mut Env, cancel: &AtomicBool) -> Summary {
        let id = begin_scan(&env.conn).unwrap();
        run(&mut env.conn, id, cancel, &Config::default(), &mut |_| {}).unwrap()
    }

    fn count_files(env: &Env) -> i64 {
        env.conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn identical_content_with_different_names_groups() {
        let mut e = env();
        write(&e, "IMG_001.jpg", b"same-bytes");
        write(&e, "sub/vacation-final.JPG", b"same-bytes");
        write(&e, "copy-of-vacation.png", b"same-bytes");
        write(&e, "other.jpg", b"diff-bytes!");
        scan(&mut e);
        let groups = duplicate_groups(&e.conn).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].files.len(), 3);
        assert_eq!(groups[0].wasted, 2 * 10);
    }

    #[test]
    fn same_size_different_content_is_not_a_group() {
        let mut e = env();
        write(&e, "a.jpg", b"aaaa");
        write(&e, "b.jpg", b"bbbb");
        let s = scan(&mut e);
        assert_eq!(s.files_hashed, 2);
        assert!(duplicate_groups(&e.conn).unwrap().is_empty());
    }

    #[test]
    fn unique_sizes_are_never_hashed() {
        let mut e = env();
        write(&e, "a.jpg", b"1");
        write(&e, "b.jpg", b"22");
        assert_eq!(scan(&mut e).files_hashed, 0);
    }

    #[test]
    fn second_scan_reuses_cached_hashes() {
        let mut e = env();
        write(&e, "a.jpg", b"dup");
        write(&e, "b.jpg", b"dup");
        assert_eq!(scan(&mut e).files_hashed, 2);
        let s = scan(&mut e);
        assert_eq!((s.files_hashed, s.files_cached), (0, 2));
    }

    #[test]
    fn modified_file_is_rehashed() {
        let mut e = env();
        let a = write(&e, "a.jpg", b"dup");
        write(&e, "b.jpg", b"dup");
        scan(&mut e);
        fs::write(&a, b"dux").unwrap(); // same size, new content
        let f = fs::File::options().write(true).open(&a).unwrap();
        f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(scan(&mut e).files_hashed, 1);
        assert!(duplicate_groups(&e.conn).unwrap().is_empty());
    }

    #[test]
    fn deleted_file_swept_only_after_completed_scan() {
        let mut e = env();
        let a = write(&e, "a.jpg", b"x1");
        write(&e, "b.jpg", b"x2");
        scan(&mut e);
        assert_eq!(count_files(&e), 2);
        fs::remove_file(&a).unwrap();

        let cancelled = AtomicBool::new(true);
        let s = scan_with(&mut e, &cancelled);
        assert_eq!(s.status, "cancelled");
        assert_eq!(count_files(&e), 2, "cancelled scan must not sweep");

        assert_eq!(scan(&mut e).status, "completed");
        assert_eq!(count_files(&e), 1);
    }

    #[test]
    fn ignores_empty_and_non_image_files() {
        let mut e = env();
        write(&e, "photo1.heic", b"heic-bytes");
        write(&e, "photo2.HEIC", b"heic-bytes");
        write(&e, "photo3.heif", b"heic-bytes");
        write(&e, "empty1.jpg", b"");
        write(&e, "empty2.jpg", b"");
        write(&e, "notes.txt", b"hello");
        write(&e, "notes2.txt", b"hello");
        assert_eq!(scan(&mut e).files_seen, 0);
        assert_eq!(count_files(&e), 0);
    }

    #[test]
    fn excluded_directories_are_pruned() {
        let mut e = env();
        write(&e, "keep.jpg", b"k");
        write(&e, "node_modules/x.jpg", b"k");
        write(&e, ".git/y.jpg", b"k");
        assert_eq!(scan(&mut e).files_seen, 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let mut e = env();
        let real = write(&e, "real.jpg", b"data");
        std::os::unix::fs::symlink(&real, e.dir.path().join("link.jpg")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("o.jpg"), b"data").unwrap();
        std::os::unix::fs::symlink(outside.path(), e.dir.path().join("linkdir")).unwrap();
        assert_eq!(scan(&mut e).files_seen, 1);
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_are_not_wasted_space() {
        let mut e = env();
        let a = write(&e, "a.jpg", b"data");
        fs::hard_link(&a, e.dir.path().join("b.jpg")).unwrap();
        scan(&mut e);
        assert!(duplicate_groups(&e.conn).unwrap().is_empty());

        write(&e, "c.jpg", b"data"); // a genuine third copy
        scan(&mut e);
        let groups = duplicate_groups(&e.conn).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].wasted, 4);
    }

    #[test]
    fn missing_folder_keeps_cached_rows() {
        let mut e = env();
        write(&e, "a.jpg", b"x");
        scan(&mut e);
        let path = e.dir.path().to_path_buf();
        fs::remove_dir_all(&path).unwrap();
        let s = scan(&mut e);
        assert_eq!(s.status, "completed");
        assert_eq!(count_files(&e), 1);
    }

    #[test]
    fn stats_report_totals() {
        let mut e = env();
        write(&e, "a.jpg", b"1234");
        write(&e, "b.jpg", b"1234");
        write(&e, "c.jpg", b"12");
        scan(&mut e);
        let s = stats(&e.conn).unwrap();
        assert_eq!((s.files, s.bytes, s.groups, s.wasted), (3, 10, 1, 4));
        assert!(s.last_scan_at.is_some());
    }

    #[test]
    fn excluded_paths_and_file_types_are_honoured() {
        let mut e = env();
        write(&e, "keep.jpg", b"k");
        write(&e, "keep.png", b"k");
        write(&e, "skip/a.jpg", b"k");
        let cfg = Config {
            excluded_paths: vec![e.dir.path().join("skip")],
            extensions: vec!["jpg".into()],
            similar: false,
        };
        let id = begin_scan(&e.conn).unwrap();
        let s = run(&mut e.conn, id, &AtomicBool::new(false), &cfg, &mut |_| {}).unwrap();
        assert_eq!(s.files_seen, 1);
    }

    #[test]
    fn config_loads_from_settings_and_ignores_unsupported_types() {
        let e = env();
        assert_eq!(Config::load(&e.conn).unwrap().extensions.len(), supported_exts().count());
        crate::db::set_setting(&e.conn, "scan.file_types", r#"["PNG","heic"]"#).unwrap();
        crate::db::set_setting(&e.conn, "scan.excluded_paths", r#"["/x/y"]"#).unwrap();
        let cfg = Config::load(&e.conn).unwrap();
        assert_eq!(cfg.extensions, vec!["png".to_string()]);
        assert_eq!(cfg.excluded_paths, vec![PathBuf::from("/x/y")]);
    }

    #[test]
    fn insights_rank_folders_and_types() {
        let mut e = env();
        write(&e, "a/1.jpg", b"1234");
        write(&e, "b/1.jpg", b"1234");
        write(&e, "a/2.png", b"12345");
        write(&e, "a/3.png", b"12345");
        scan(&mut e);
        let i = insights(&e.conn).unwrap();
        assert_eq!(i.folders[0].count, 3);
        assert!(i.folders[0].label.ends_with('a'));
        assert_eq!(i.types.len(), 2);
        assert_eq!(i.types.iter().map(|t| t.count).sum::<i64>(), 4);
    }

    #[test]
    fn videos_audio_documents_and_archives_group_by_content() {
        let mut e = env();
        for (a, b, body) in [
            ("clip.mp4", "backup/clip-copy.MKV", b"video-bytes".as_slice()),
            ("song.mp3", "music/song (1).flac", b"audio-bytes"),
            ("report.pdf", "old/report-final.pdf", b"pdf-bytes!"),
            ("data.zip", "data2.7z", b"archive-bytes"),
        ] {
            write(&e, a, body);
            write(&e, b, body);
        }
        write(&e, "notes.txt", b"video-bytes"); // not a supported type: ignored
        let s = scan(&mut e);
        assert_eq!(s.files_seen, 8);
        let groups = duplicate_groups(&e.conn).unwrap();
        assert_eq!(groups.len(), 4);
        assert!(groups.iter().all(|g| g.files.len() == 2));
    }

    #[test]
    fn only_images_are_fingerprinted_or_measured() {
        let mut e = env();
        write(&e, "a.mp4", b"same");
        write(&e, "b.mp4", b"same");
        let s = scan(&mut e);
        assert_eq!(s.files_fingerprinted, 0);
        let mut groups = duplicate_groups(&e.conn).unwrap();
        crate::review::enrich(&e.conn, &mut groups).unwrap();
        assert_eq!(groups[0].files[0].width, None);
        assert!(groups[0].keep_id > 0, "a copy is still recommended");
    }
}

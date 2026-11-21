//! Safe cleanup: quarantine, restore and permanent deletion of duplicate files.
//!
//! Nothing here runs on its own; every function acts only on ids the user passed in.
//! The rules that protect the user's data live here, not in the UI:
//! * a file is only removed if its hash is known and it is unchanged since the scan;
//! * at least one other copy, also unchanged, must remain on disk (for similar images: a
//!   surviving image whose fingerprint is within the similarity threshold of each removed one);
//! * quarantined files are moved (not copied), and can be restored to where they came from.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::scan::{file_id, mtime_ns};

const RECOVERED_KEY: &str = "stats.recovered_bytes";
const DIR_KEY: &str = "quarantine.dir";

#[derive(Debug, Serialize, PartialEq)]
pub struct Failure {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Default, PartialEq)]
pub struct CleanupResult {
    pub done: u32,
    pub bytes: i64,
    pub failed: Vec<Failure>,
}

impl CleanupResult {
    fn fail(&mut self, path: &str, reason: &str) {
        self.failed.push(Failure { path: path.to_owned(), reason: reason.to_owned() });
    }
}

/// Where new quarantined files go: the user's choice, else `default`.
pub fn dir(conn: &Connection, default: &Path) -> Result<PathBuf, String> {
    Ok(match crate::db::get_setting(conn, DIR_KEY)? {
        Some(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
        _ => default.to_path_buf(),
    })
}

/// Saves a new quarantine location after checking it is usable. An empty `input` restores
/// the default. Files already in quarantine stay where they are (their path is stored).
pub fn set_dir(conn: &Connection, default: &Path, input: &str) -> Result<PathBuf, String> {
    let input = input.trim();
    if input.is_empty() {
        crate::db::set_setting(conn, DIR_KEY, "")?;
        return Ok(default.to_path_buf());
    }
    let path = PathBuf::from(input);
    if !path.is_absolute() {
        return Err("Enter a full (absolute) path".into());
    }
    fs::create_dir_all(&path).map_err(|e| format!("Can't use that folder: {e}"))?;
    let probe = path.join(".sift-write-test");
    fs::write(&probe, b"").map_err(|e| format!("That folder isn't writable: {e}"))?;
    let _ = fs::remove_file(&probe);
    crate::db::set_setting(conn, DIR_KEY, input)?;
    Ok(path)
}

/// What the user was looking at when they chose files: byte-identical copies, or images that
/// merely look alike. It decides what "another copy remains" means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Exact,
    Similar,
}

impl Mode {
    pub fn parse(s: Option<&str>) -> Result<Mode, String> {
        match s {
            None | Some("exact") => Ok(Mode::Exact),
            Some("similar") => Ok(Mode::Similar),
            Some(other) => Err(format!("unknown cleanup mode \"{other}\"")),
        }
    }
}

struct Tracked {
    id: i64,
    path: String,
    size: i64,
    mtime: i64,
    sha: Option<String>,
}

fn load(conn: &Connection, id: i64) -> Result<Option<Tracked>, String> {
    conn.query_row(
        "SELECT id, path, size, mtime_ns, sha256 FROM files WHERE id = ?1",
        params![id],
        |r| Ok(Tracked { id: r.get(0)?, path: r.get(1)?, size: r.get(2)?, mtime: r.get(3)?, sha: r.get(4)? }),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// True if the file is still the one that was hashed.
fn unchanged(t: &Tracked) -> bool {
    fs::metadata(&t.path)
        .map(|m| m.is_file() && m.len() as i64 == t.size && mtime_ns(&m) == t.mtime)
        .unwrap_or(false)
}

/// Validates a removal request and returns the files that may be removed.
/// Anything refused is reported in `result.failed`.
fn plan(conn: &Connection, ids: &[i64], mode: Mode, result: &mut CleanupResult) -> Result<Vec<Tracked>, String> {
    match mode {
        Mode::Exact => plan_exact(conn, ids, result),
        Mode::Similar => plan_similar(conn, ids, result),
    }
}

/// Similar-image removal: each file must have a fingerprint, be unchanged, and have a
/// surviving (unchanged, not selected) image within the similarity threshold. Chains don't
/// count: a removed image must be close to a survivor itself, not to another removed one.
fn plan_similar(conn: &Connection, ids: &[i64], result: &mut CleanupResult) -> Result<Vec<Tracked>, String> {
    let max_dist = crate::similar::threshold(conn)?;
    let mut seen = HashSet::new();
    let mut candidates: Vec<(Tracked, u64)> = Vec::new();
    for &id in ids {
        if !seen.insert(id) {
            continue;
        }
        let Some(t) = load(conn, id)? else {
            result.fail(&format!("file #{id}"), "not tracked by Sift");
            continue;
        };
        let ph: Option<i64> = conn
            .query_row("SELECT phash FROM files WHERE id = ?1", params![id], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        match ph {
            None => result.fail(&t.path, "not confirmed as similar (no fingerprint)"),
            Some(_) if !unchanged(&t) => {
                result.fail(&t.path, "missing or changed since the last scan; rescan first")
            }
            Some(p) => candidates.push((t, p as u64)),
        }
    }
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let removing: HashSet<i64> = candidates.iter().map(|(t, _)| t.id).collect();
    let survivors: Vec<(Tracked, u64)> = {
        let mut stmt = conn
            .prepare("SELECT id, path, size, mtime_ns, sha256, phash FROM files WHERE phash IS NOT NULL")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    Tracked { id: r.get(0)?, path: r.get(1)?, size: r.get(2)?, mtime: r.get(3)?, sha: r.get(4)? },
                    r.get::<_, i64>(5)? as u64,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };

    let mut ok = Vec::new();
    for (t, ph) in candidates {
        let has_keeper = survivors.iter().any(|(s, sp)| {
            !removing.contains(&s.id) && crate::similar::hamming(ph, *sp) <= max_dist && unchanged(s)
        });
        if has_keeper {
            ok.push(t);
        } else {
            result.fail(&t.path, "no similar image would remain");
        }
    }

    // Quarantine records a content hash so a restored file can be recognised again, and
    // similar-only images were never hashed (their sizes are unique). Hash them now.
    let never = AtomicBool::new(false);
    let mut verified = Vec::new();
    for mut t in ok {
        if t.sha.is_none() {
            match crate::scan::hash_file(&t.path, &never) {
                Ok(Some(h)) if unchanged(&t) => {
                    conn.execute("UPDATE files SET sha256 = ?2 WHERE id = ?1", params![t.id, h])
                        .map_err(|e| e.to_string())?;
                    t.sha = Some(h);
                }
                Ok(_) => {
                    result.fail(&t.path, "missing or changed since the last scan; rescan first");
                    continue;
                }
                Err(e) => {
                    result.fail(&t.path, &e.to_string());
                    continue;
                }
            }
        }
        verified.push(t);
    }
    verified.sort_by_key(|t| t.id);
    Ok(verified)
}

fn plan_exact(conn: &Connection, ids: &[i64], result: &mut CleanupResult) -> Result<Vec<Tracked>, String> {
    let mut seen = HashSet::new();
    let mut candidates: Vec<Tracked> = Vec::new();
    for &id in ids {
        if !seen.insert(id) {
            continue;
        }
        let Some(t) = load(conn, id)? else {
            result.fail(&format!("file #{id}"), "not tracked by Sift");
            continue;
        };
        if t.sha.is_none() {
            result.fail(&t.path, "not confirmed as a duplicate (no hash)");
        } else if !unchanged(&t) {
            result.fail(&t.path, "missing or changed since the last scan; rescan first");
        } else {
            candidates.push(t);
        }
    }

    let removing: HashSet<i64> = candidates.iter().map(|t| t.id).collect();
    let mut by_sha: HashMap<String, Vec<Tracked>> = HashMap::new();
    for t in candidates {
        by_sha.entry(t.sha.clone().unwrap()).or_default().push(t);
    }

    let mut ok = Vec::new();
    for (sha, group) in by_sha {
        let mut stmt = conn
            .prepare_cached("SELECT id, path, size, mtime_ns, sha256 FROM files WHERE sha256 = ?1")
            .map_err(|e| e.to_string())?;
        let all = stmt
            .query_map(params![sha], |r| {
                Ok(Tracked { id: r.get(0)?, path: r.get(1)?, size: r.get(2)?, mtime: r.get(3)?, sha: r.get(4)? })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let keeper = all.iter().any(|t| !removing.contains(&t.id) && unchanged(t));
        if keeper {
            ok.extend(group);
        } else {
            for t in group {
                result.fail(&t.path, "would remove the last remaining copy");
            }
        }
    }
    ok.sort_by_key(|t| t.id);
    Ok(ok)
}

/// Renames, falling back to copy + verify + delete when crossing filesystems.
fn move_file(src: &Path, dest: &Path) -> Result<(), String> {
    if fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    let expected = fs::metadata(src).map_err(|e| e.to_string())?.len();
    fs::copy(src, dest).map_err(|e| e.to_string())?;
    if fs::metadata(dest).map(|m| m.len()).ok() != Some(expected) {
        let _ = fs::remove_file(dest);
        return Err("copy did not complete".into());
    }
    if let Err(e) = fs::remove_file(src) {
        let _ = fs::remove_file(dest);
        return Err(e.to_string());
    }
    Ok(())
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into())
}

/// Moves the files into `qdir` and forgets them in the files table.
pub fn quarantine(conn: &Connection, qdir: &Path, ids: &[i64], mode: Mode) -> Result<CleanupResult, String> {
    let mut result = CleanupResult::default();
    let files = plan(conn, ids, mode, &mut result)?;
    if files.is_empty() {
        return Ok(result);
    }
    fs::create_dir_all(qdir).map_err(|e| e.to_string())?;

    for t in files {
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let inserted = tx.execute(
            "INSERT INTO quarantine (original_path, stored_path, size, sha256, width, height)
             SELECT path, '', size, sha256, width, height FROM files WHERE id = ?1",
            params![t.id],
        );
        if let Err(e) = inserted {
            result.fail(&t.path, &e.to_string());
            continue;
        }
        let qid = tx.last_insert_rowid();
        let dest = qdir.join(format!("{qid}_{}", file_name(&t.path)));
        if let Err(e) = move_file(Path::new(&t.path), &dest) {
            result.fail(&t.path, &format!("could not move: {e}"));
            continue; // dropping `tx` rolls the row back
        }
        let stored = dest.to_string_lossy().into_owned();
        let committed = tx
            .execute("UPDATE quarantine SET stored_path = ?2 WHERE id = ?1", params![qid, stored])
            .and_then(|_| tx.execute("DELETE FROM files WHERE id = ?1", params![t.id]))
            .and_then(|_| tx.commit());
        if let Err(e) = committed {
            let _ = move_file(&dest, Path::new(&t.path)); // put it back
            result.fail(&t.path, &e.to_string());
            continue;
        }
        result.done += 1;
        result.bytes += t.size;
    }
    Ok(result)
}

/// Permanently deletes duplicate files (same safety rules as quarantine).
pub fn delete_files(conn: &Connection, ids: &[i64], mode: Mode) -> Result<CleanupResult, String> {
    let mut result = CleanupResult::default();
    for t in plan(conn, ids, mode, &mut result)? {
        if let Err(e) = fs::remove_file(&t.path) {
            result.fail(&t.path, &e.to_string());
            continue;
        }
        conn.execute("DELETE FROM files WHERE id = ?1", params![t.id]).map_err(|e| e.to_string())?;
        result.done += 1;
        result.bytes += t.size;
    }
    add_recovered(conn, result.bytes)?;
    Ok(result)
}

#[derive(Debug, Serialize, PartialEq)]
pub struct QuarantineItem {
    pub id: i64,
    pub original_path: String,
    pub size: i64,
    /// Unix seconds.
    pub moved_at: i64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// False if the stored file has vanished (it can then only be forgotten).
    pub exists: bool,
}

pub fn list_quarantine(conn: &Connection) -> Result<Vec<QuarantineItem>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, original_path, stored_path, size, moved_at, width, height
             FROM quarantine ORDER BY moved_at DESC, id DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let stored: String = r.get(2)?;
            Ok(QuarantineItem {
                id: r.get(0)?,
                original_path: r.get(1)?,
                size: r.get(3)?,
                moved_at: r.get(4)?,
                width: r.get(5)?,
                height: r.get(6)?,
                exists: Path::new(&stored).is_file(),
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// `photo.jpg` → `photo (restored).jpg` → `photo (restored 2).jpg` …
fn unique_path(original: &Path) -> PathBuf {
    if !original.exists() {
        return original.to_path_buf();
    }
    let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = original.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = original.parent().unwrap_or(Path::new(""));
    (1..)
        .map(|n| {
            let tag = if n == 1 { "restored".to_string() } else { format!("restored {n}") };
            dir.join(format!("{stem} ({tag}){ext}"))
        })
        .find(|p| !p.exists())
        .unwrap()
}

/// Moves quarantined files back to their original location. An existing file is never
/// overwritten: the restored copy gets a " (restored)" suffix instead.
pub fn restore(conn: &Connection, ids: &[i64]) -> Result<CleanupResult, String> {
    let mut result = CleanupResult::default();
    let roots = crate::db::folder_rows(conn)?;
    for &id in ids {
        let row = conn
            .query_row(
                "SELECT original_path, stored_path, size, sha256, width, height FROM quarantine WHERE id = ?1",
                params![id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<u32>>(4)?,
                        r.get::<_, Option<u32>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some((original, stored, size, sha, w, h)) = row else {
            result.fail(&format!("item #{id}"), "not in quarantine");
            continue;
        };
        if !Path::new(&stored).is_file() {
            result.fail(&original, "the quarantined file is missing");
            continue;
        }
        let dest = unique_path(Path::new(&original));
        if let Some(parent) = dest.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                result.fail(&original, &format!("could not create folder: {e}"));
                continue;
            }
        }
        if let Err(e) = move_file(Path::new(&stored), &dest) {
            result.fail(&original, &format!("could not restore: {e}"));
            continue;
        }
        conn.execute("DELETE FROM quarantine WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;

        // Track it again right away when it lands inside a scanned folder.
        let owner = roots
            .iter()
            .filter(|(_, root)| dest.starts_with(root))
            .max_by_key(|(_, root)| root.len());
        if let (Some((folder_id, _)), Ok(md)) = (owner, fs::metadata(&dest)) {
            conn.execute(
                "INSERT OR IGNORE INTO files
                    (folder_id, path, size, mtime_ns, file_id, sha256, width, height, last_seen_scan)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, (SELECT COALESCE(MAX(id), 0) FROM scans))",
                params![
                    folder_id,
                    dest.to_string_lossy(),
                    size,
                    mtime_ns(&md),
                    file_id(&md),
                    sha,
                    w,
                    h
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        result.done += 1;
        result.bytes += size;
    }
    Ok(result)
}

/// Permanently deletes quarantined files. An empty `ids` slice is not "everything".
pub fn delete_quarantined(conn: &Connection, ids: &[i64]) -> Result<CleanupResult, String> {
    let mut result = CleanupResult::default();
    for &id in ids {
        let row: Option<(String, String, i64)> = conn
            .query_row(
                "SELECT original_path, stored_path, size FROM quarantine WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some((original, stored, size)) = row else {
            result.fail(&format!("item #{id}"), "not in quarantine");
            continue;
        };
        match fs::remove_file(&stored) {
            Ok(()) => result.bytes += size,
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => {
                result.fail(&original, &e.to_string());
                continue;
            }
        }
        conn.execute("DELETE FROM quarantine WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
        result.done += 1;
    }
    add_recovered(conn, result.bytes)?;
    Ok(result)
}

fn add_recovered(conn: &Connection, bytes: i64) -> Result<(), String> {
    if bytes == 0 {
        return Ok(());
    }
    let total = recovered(conn)? + bytes;
    crate::db::set_setting(conn, RECOVERED_KEY, &total.to_string())
}

/// Bytes permanently freed so far (permanent deletes and emptied quarantine).
pub fn recovered(conn: &Connection) -> Result<i64, String> {
    Ok(crate::db::get_setting(conn, RECOVERED_KEY)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0))
}

/// Bytes currently held in quarantine.
pub fn quarantined_bytes(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT COALESCE(SUM(size), 0) FROM quarantine", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    struct Env {
        conn: Connection,
        dir: tempfile::TempDir,
        q: tempfile::TempDir,
    }

    fn env() -> Env {
        let conn = crate::db::init(Connection::open_in_memory().unwrap()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::db::add_folder(&conn, dir.path().to_str().unwrap()).unwrap();
        Env { conn, dir, q: tempfile::tempdir().unwrap() }
    }

    fn write(e: &Env, name: &str, content: &[u8]) -> PathBuf {
        let p = e.dir.path().join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }

    fn scan(e: &mut Env) {
        let id = crate::scan::begin_scan(&e.conn).unwrap();
        crate::scan::run(&mut e.conn, id, &AtomicBool::new(false), &crate::scan::Config::default(), &mut |_| {})
            .unwrap();
    }

    fn id_of(e: &Env, p: &Path) -> i64 {
        e.conn
            .query_row("SELECT id FROM files WHERE path = ?1", params![p.to_str().unwrap()], |r| r.get(0))
            .unwrap()
    }

    fn tracked_count(e: &Env) -> i64 {
        e.conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn quarantine_moves_file_and_untracks_it() {
        let mut e = env();
        let a = write(&e, "a.jpg", b"data");
        let b = write(&e, "b.jpg", b"data");
        scan(&mut e);
        let r = quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)], Mode::Exact).unwrap();
        assert_eq!((r.done, r.bytes, r.failed.len()), (1, 4, 0));
        assert!(a.exists() && !b.exists());
        assert_eq!(tracked_count(&e), 1);
        let items = list_quarantine(&e.conn).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].exists);
        assert_eq!(quarantined_bytes(&e.conn).unwrap(), 4);
    }

    #[test]
    fn refuses_to_remove_every_copy() {
        let mut e = env();
        let a = write(&e, "a.jpg", b"data");
        let b = write(&e, "b.jpg", b"data");
        scan(&mut e);
        let ids = [id_of(&e, &a), id_of(&e, &b)];
        let r = quarantine(&e.conn, e.q.path(), &ids, Mode::Exact).unwrap();
        assert_eq!(r.done, 0);
        assert_eq!(r.failed.len(), 2);
        assert!(a.exists() && b.exists());
        let r = delete_files(&e.conn, &ids, Mode::Exact).unwrap();
        assert_eq!(r.done, 0);
        assert!(a.exists() && b.exists());
    }

    #[test]
    fn refuses_files_changed_since_scan() {
        let mut e = env();
        let a = write(&e, "a.jpg", b"data");
        let b = write(&e, "b.jpg", b"data");
        scan(&mut e);
        fs::write(&b, b"changed!").unwrap();
        let r = quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)], Mode::Exact).unwrap();
        assert_eq!(r.done, 0);
        assert!(b.exists());

        // A changed keeper doesn't count as a surviving copy either.
        let c = write(&e, "c.jpg", b"data2");
        let d = write(&e, "d.jpg", b"data2");
        scan(&mut e);
        fs::write(&c, b"other").unwrap();
        let r = quarantine(&e.conn, e.q.path(), &[id_of(&e, &d)], Mode::Exact).unwrap();
        assert_eq!(r.done, 0);
        assert!(d.exists() && a.exists());
    }

    #[test]
    fn unknown_ids_are_reported_not_fatal() {
        let e = env();
        let r = quarantine(&e.conn, e.q.path(), &[12345], Mode::Exact).unwrap();
        assert_eq!((r.done, r.failed.len()), (0, 1));
    }

    #[test]
    fn restore_puts_file_back_and_retracks_it() {
        let mut e = env();
        write(&e, "a.jpg", b"data");
        let b = write(&e, "sub/b.jpg", b"data");
        scan(&mut e);
        quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)], Mode::Exact).unwrap();
        fs::remove_dir(e.dir.path().join("sub")).unwrap(); // folder gone too
        let qid = list_quarantine(&e.conn).unwrap()[0].id;
        let r = restore(&e.conn, &[qid]).unwrap();
        assert_eq!((r.done, r.failed.len()), (1, 0));
        assert!(b.exists());
        assert!(list_quarantine(&e.conn).unwrap().is_empty());
        assert_eq!(tracked_count(&e), 2);
        assert_eq!(crate::scan::duplicate_groups(&e.conn).unwrap().len(), 1);
    }

    #[test]
    fn restore_never_overwrites() {
        let mut e = env();
        write(&e, "a.jpg", b"data");
        let b = write(&e, "b.jpg", b"data");
        scan(&mut e);
        quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)], Mode::Exact).unwrap();
        fs::write(&b, b"someone else's file").unwrap();
        let qid = list_quarantine(&e.conn).unwrap()[0].id;
        restore(&e.conn, &[qid]).unwrap();
        assert_eq!(fs::read(&b).unwrap(), b"someone else's file");
        assert_eq!(fs::read(e.dir.path().join("b (restored).jpg")).unwrap(), b"data");
    }

    #[test]
    fn permanent_delete_counts_as_recovered() {
        let mut e = env();
        write(&e, "a.jpg", b"data");
        let b = write(&e, "b.jpg", b"data");
        scan(&mut e);
        let r = delete_files(&e.conn, &[id_of(&e, &b)], Mode::Exact).unwrap();
        assert_eq!(r.done, 1);
        assert!(!b.exists());
        assert_eq!(tracked_count(&e), 1);
        assert_eq!(recovered(&e.conn).unwrap(), 4);
    }

    #[test]
    fn deleting_quarantined_files_frees_space() {
        let mut e = env();
        write(&e, "a.jpg", b"data");
        let b = write(&e, "b.jpg", b"data");
        scan(&mut e);
        quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)], Mode::Exact).unwrap();
        assert_eq!(recovered(&e.conn).unwrap(), 0, "quarantine alone frees nothing");
        let qid = list_quarantine(&e.conn).unwrap()[0].id;
        let r = delete_quarantined(&e.conn, &[qid]).unwrap();
        assert_eq!(r.done, 1);
        assert!(list_quarantine(&e.conn).unwrap().is_empty());
        assert_eq!(fs::read_dir(e.q.path()).unwrap().count(), 0);
        assert_eq!(recovered(&e.conn).unwrap(), 4);
    }

    #[test]
    fn quarantine_dir_is_configurable_with_default_fallback() {
        let e = env();
        let default = e.q.path().join("default");
        assert_eq!(dir(&e.conn, &default).unwrap(), default);

        let custom = e.q.path().join("custom/nested");
        assert_eq!(set_dir(&e.conn, &default, custom.to_str().unwrap()).unwrap(), custom);
        assert!(custom.is_dir());
        assert_eq!(dir(&e.conn, &default).unwrap(), custom);

        assert!(set_dir(&e.conn, &default, "relative/path").is_err());
        let file = e.q.path().join("a_file");
        fs::write(&file, b"x").unwrap();
        assert!(set_dir(&e.conn, &default, file.to_str().unwrap()).is_err());
        assert_eq!(dir(&e.conn, &default).unwrap(), custom, "failed changes keep the old value");

        assert_eq!(set_dir(&e.conn, &default, "  ").unwrap(), default);
        assert_eq!(dir(&e.conn, &default).unwrap(), default);
    }

    fn save_picture(e: &Env, name: &str, seed: u32, size: u32) -> PathBuf {
        let p = e.dir.path().join(name);
        crate::similar::test_picture(seed, size).save(&p).unwrap();
        p
    }

    fn similar_env() -> (Env, PathBuf, PathBuf, PathBuf) {
        let mut e = env();
        let big = save_picture(&e, "big.png", 1, 200);
        let small = e.dir.path().join("small.png");
        image::open(&big)
            .unwrap()
            .resize_exact(90, 90, image::imageops::FilterType::Lanczos3)
            .save(&small)
            .unwrap();
        let other = save_picture(&e, "other.png", 4, 200);
        scan(&mut e);
        (e, big, small, other)
    }

    #[test]
    fn similar_mode_quarantines_when_a_similar_image_remains() {
        let (e, big, small, _) = similar_env();
        // The files have different sizes, so the exact pipeline never hashed them.
        let sha: Option<String> = e
            .conn
            .query_row("SELECT sha256 FROM files WHERE id = ?1", params![id_of(&e, &small)], |r| r.get(0))
            .unwrap();
        assert!(sha.is_none());

        let r = quarantine(&e.conn, e.q.path(), &[id_of(&e, &small)], Mode::Similar).unwrap();
        assert_eq!((r.done, r.failed.len()), (1, 0), "{:?}", r.failed);
        assert!(!small.exists() && big.exists());
        assert_eq!(list_quarantine(&e.conn).unwrap().len(), 1);
    }

    #[test]
    fn similar_mode_refuses_to_remove_every_similar_image() {
        let (e, big, small, _) = similar_env();
        let r = quarantine(&e.conn, e.q.path(), &[id_of(&e, &big), id_of(&e, &small)], Mode::Similar).unwrap();
        assert_eq!(r.done, 2 - r.failed.len() as u32);
        assert_eq!(r.failed.len(), 2, "{:?}", r.failed);
        assert!(big.exists() && small.exists());
    }

    #[test]
    fn similar_mode_refuses_an_image_with_no_similar_survivor() {
        let (e, _, _, other) = similar_env();
        let r = delete_files(&e.conn, &[id_of(&e, &other)], Mode::Similar).unwrap();
        assert_eq!(r.done, 0);
        assert_eq!(r.failed.len(), 1);
        assert!(other.exists());
    }

    #[test]
    fn similar_mode_refuses_files_changed_since_scan() {
        let (e, big, small, _) = similar_env();
        fs::write(&small, b"changed").unwrap();
        let r = delete_files(&e.conn, &[id_of(&e, &small)], Mode::Similar).unwrap();
        assert_eq!(r.done, 0);
        assert!(small.exists() && big.exists());
    }

    #[test]
    fn similar_mode_does_not_count_a_changed_survivor() {
        let (e, big, small, _) = similar_env();
        fs::write(&big, b"changed").unwrap();
        let r = delete_files(&e.conn, &[id_of(&e, &small)], Mode::Similar).unwrap();
        assert_eq!(r.done, 0, "the only similar image changed on disk, so nothing is safe to remove");
        assert!(small.exists());
    }

    #[test]
    fn modes_parse() {
        assert_eq!(Mode::parse(None).unwrap(), Mode::Exact);
        assert_eq!(Mode::parse(Some("similar")).unwrap(), Mode::Similar);
        assert!(Mode::parse(Some("whatever")).is_err());
    }
}

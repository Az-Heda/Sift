//! Safe cleanup: quarantine, restore and permanent deletion of duplicate files.
//!
//! Nothing here runs on its own; every function acts only on ids the user passed in.
//! The rules that protect the user's data live here, not in the UI:
//! * a file is only removed if its hash is known and it is unchanged since the scan;
//! * at least one other copy, also unchanged, must remain on disk;
//! * quarantined files are moved (not copied), and can be restored to where they came from.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

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
fn plan(conn: &Connection, ids: &[i64], result: &mut CleanupResult) -> Result<Vec<Tracked>, String> {
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
pub fn quarantine(conn: &Connection, qdir: &Path, ids: &[i64]) -> Result<CleanupResult, String> {
    let mut result = CleanupResult::default();
    let files = plan(conn, ids, &mut result)?;
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
pub fn delete_files(conn: &Connection, ids: &[i64]) -> Result<CleanupResult, String> {
    let mut result = CleanupResult::default();
    for t in plan(conn, ids, &mut result)? {
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
        let r = quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)]).unwrap();
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
        let r = quarantine(&e.conn, e.q.path(), &ids).unwrap();
        assert_eq!(r.done, 0);
        assert_eq!(r.failed.len(), 2);
        assert!(a.exists() && b.exists());
        let r = delete_files(&e.conn, &ids).unwrap();
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
        let r = quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)]).unwrap();
        assert_eq!(r.done, 0);
        assert!(b.exists());

        // A changed keeper doesn't count as a surviving copy either.
        let c = write(&e, "c.jpg", b"data2");
        let d = write(&e, "d.jpg", b"data2");
        scan(&mut e);
        fs::write(&c, b"other").unwrap();
        let r = quarantine(&e.conn, e.q.path(), &[id_of(&e, &d)]).unwrap();
        assert_eq!(r.done, 0);
        assert!(d.exists() && a.exists());
    }

    #[test]
    fn unknown_ids_are_reported_not_fatal() {
        let e = env();
        let r = quarantine(&e.conn, e.q.path(), &[12345]).unwrap();
        assert_eq!((r.done, r.failed.len()), (0, 1));
    }

    #[test]
    fn restore_puts_file_back_and_retracks_it() {
        let mut e = env();
        write(&e, "a.jpg", b"data");
        let b = write(&e, "sub/b.jpg", b"data");
        scan(&mut e);
        quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)]).unwrap();
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
        quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)]).unwrap();
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
        let r = delete_files(&e.conn, &[id_of(&e, &b)]).unwrap();
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
        quarantine(&e.conn, e.q.path(), &[id_of(&e, &b)]).unwrap();
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
}

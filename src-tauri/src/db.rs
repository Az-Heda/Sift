use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Ordered list of schema migrations. Index + 1 == the `user_version` it brings the DB to.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE folders (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        path        TEXT NOT NULL UNIQUE,
        added_at    INTEGER NOT NULL DEFAULT (unixepoch())
    );
    CREATE TABLE settings (
        key     TEXT PRIMARY KEY,
        value   TEXT NOT NULL
    );",
    "CREATE TABLE scans (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        started_at    INTEGER NOT NULL DEFAULT (unixepoch()),
        finished_at   INTEGER,
        status        TEXT NOT NULL DEFAULT 'running',
        files_seen    INTEGER NOT NULL DEFAULT 0,
        files_hashed  INTEGER NOT NULL DEFAULT 0,
        files_cached  INTEGER NOT NULL DEFAULT 0,
        errors        INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE files (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
        path            TEXT NOT NULL UNIQUE,
        size            INTEGER NOT NULL,
        mtime_ns        INTEGER NOT NULL,
        file_id         TEXT,
        sha256          TEXT,
        last_seen_scan  INTEGER NOT NULL
    );
    CREATE INDEX files_size ON files(size);
    CREATE INDEX files_sha ON files(sha256);
    CREATE INDEX files_folder ON files(folder_id);
    ALTER TABLE folders ADD COLUMN last_scanned_at INTEGER;",
];

#[derive(Debug, Serialize, PartialEq)]
pub struct Folder {
    pub id: i64,
    pub path: String,
    pub added_at: i64,
    pub last_scanned_at: Option<i64>,
    pub file_count: i64,
    /// False when the folder no longer exists on disk (deleted, unmounted drive...).
    pub exists: bool,
}

pub fn open(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    init(conn)
}

pub fn init(conn: Connection) -> Result<Connection, String> {
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
        .map_err(|e| e.to_string())?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), String> {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        conn.execute_batch(&format!("BEGIN; {sql} PRAGMA user_version = {}; COMMIT;", i + 1))
            .map_err(|e| format!("migration {} failed: {e}", i + 1))?;
    }
    Ok(())
}

pub fn list_folders(conn: &Connection) -> Result<Vec<Folder>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT f.id, f.path, f.added_at, f.last_scanned_at,
                    (SELECT COUNT(*) FROM files x WHERE x.folder_id = f.id)
             FROM folders f ORDER BY f.path COLLATE NOCASE",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let path: String = r.get(1)?;
            Ok(Folder {
                id: r.get(0)?,
                exists: Path::new(&path).is_dir(),
                path,
                added_at: r.get(2)?,
                last_scanned_at: r.get(3)?,
                file_count: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

pub fn add_folder(conn: &Connection, raw: &str) -> Result<Folder, String> {
    let canonical: PathBuf = dunce::canonicalize(raw)
        .map_err(|e| format!("Cannot access \"{raw}\": {e}"))?;
    if !canonical.is_dir() {
        return Err(format!("\"{}\" is not a folder", canonical.display()));
    }

    for existing in list_folders(conn)? {
        let existing_path = Path::new(&existing.path);
        if canonical == existing_path {
            return Err("This folder was already added".into());
        }
        if canonical.starts_with(existing_path) {
            return Err(format!("Already covered by \"{}\"", existing.path));
        }
        if existing_path.starts_with(&canonical) {
            return Err(format!(
                "Contains \"{}\", which is already added. Remove it first.",
                existing.path
            ));
        }
    }

    let path = canonical.to_string_lossy().into_owned();
    conn.execute("INSERT INTO folders (path) VALUES (?1)", params![path])
        .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();
    let added_at = conn
        .query_row("SELECT added_at FROM folders WHERE id = ?1", params![id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(Folder { id, path, added_at, last_scanned_at: None, file_count: 0, exists: true })
}

/// (id, path) of every tracked folder.
pub fn folder_rows(conn: &Connection) -> Result<Vec<(i64, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT id, path FROM folders ORDER BY id")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

pub fn remove_folder(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM folders WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        init(Connection::open_in_memory().unwrap()).unwrap()
    }

    #[test]
    fn migrations_are_idempotent() {
        let conn = mem();
        migrate(&conn).unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
    }

    #[test]
    fn add_list_remove_folder() {
        let conn = mem();
        let dir = tempfile::tempdir().unwrap();
        let f = add_folder(&conn, dir.path().to_str().unwrap()).unwrap();
        assert!(f.exists);
        assert_eq!(list_folders(&conn).unwrap().len(), 1);
        remove_folder(&conn, f.id).unwrap();
        assert!(list_folders(&conn).unwrap().is_empty());
    }

    #[test]
    fn rejects_duplicates_and_nesting() {
        let conn = mem();
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();

        add_folder(&conn, root.path().to_str().unwrap()).unwrap();
        assert!(add_folder(&conn, root.path().to_str().unwrap()).is_err());
        assert!(add_folder(&conn, child.to_str().unwrap()).is_err());

        let conn2 = mem();
        add_folder(&conn2, child.to_str().unwrap()).unwrap();
        assert!(add_folder(&conn2, root.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn rejects_files_and_missing_paths() {
        let conn = mem();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(add_folder(&conn, file.to_str().unwrap()).is_err());
        assert!(add_folder(&conn, dir.path().join("nope").to_str().unwrap()).is_err());
    }

    #[test]
    fn removing_folder_cascades_to_files() {
        let conn = mem();
        let dir = tempfile::tempdir().unwrap();
        let f = add_folder(&conn, dir.path().to_str().unwrap()).unwrap();
        conn.execute(
            "INSERT INTO files (folder_id, path, size, mtime_ns, last_seen_scan) VALUES (?1, 'x', 1, 1, 1)",
            params![f.id],
        )
        .unwrap();
        remove_folder(&conn, f.id).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn settings_upsert() {
        let conn = mem();
        assert_eq!(get_setting(&conn, "theme").unwrap(), None);
        set_setting(&conn, "theme", "a").unwrap();
        set_setting(&conn, "theme", "b").unwrap();
        assert_eq!(get_setting(&conn, "theme").unwrap().as_deref(), Some("b"));
    }
}

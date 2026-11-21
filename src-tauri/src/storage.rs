//! Storage analytics (V3): where the space goes, from the files Sift has already scanned.
//! Everything here is read-only; nothing is ever removed from these lists automatically.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;
use walkdir::WalkDir;

use crate::scan::{category_of, Config};

const TOP: usize = 10;
/// Files untouched for this long count as "old".
const OLD_SECONDS: i64 = 2 * 365 * 24 * 3600;
const MAX_EMPTY: usize = 200;

#[derive(Debug, Serialize, PartialEq)]
pub struct CategoryUsage {
    pub category: &'static str,
    pub files: i64,
    pub bytes: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct StoredFile {
    pub id: i64,
    pub path: String,
    pub size: i64,
    /// Last modified, Unix seconds.
    pub modified: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FolderUsage {
    pub path: String,
    pub files: i64,
    pub bytes: i64,
}

#[derive(Debug, Serialize, Default, PartialEq)]
pub struct Analytics {
    pub categories: Vec<CategoryUsage>,
    pub largest: Vec<StoredFile>,
    pub folders: Vec<FolderUsage>,
    /// Biggest files not modified for two years.
    pub old: Vec<StoredFile>,
}

fn stored_files(conn: &Connection, sql: &str) -> Result<Vec<StoredFile>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(StoredFile {
                id: r.get(0)?,
                path: r.get(1)?,
                size: r.get(2)?,
                modified: r.get::<_, i64>(3)? / 1_000_000_000,
            })
        })
        .map_err(|e| e.to_string())?;
    let all: Vec<StoredFile> = rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    // The table can lag behind the disk: only list files that still exist.
    Ok(all.into_iter().filter(|f| Path::new(&f.path).is_file()).take(TOP).collect())
}

pub fn analytics(conn: &Connection) -> Result<Analytics, String> {
    let mut by_cat: HashMap<&'static str, (i64, i64)> = HashMap::new();
    let mut by_dir: HashMap<String, (i64, i64)> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT path, size FROM files").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (path, size) = row.map_err(|e| e.to_string())?;
            let p = Path::new(&path);
            let c = by_cat.entry(category_of(p)).or_default();
            c.0 += 1;
            c.1 += size;
            let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
            let d = by_dir.entry(dir).or_default();
            d.0 += 1;
            d.1 += size;
        }
    }

    let mut categories: Vec<CategoryUsage> = by_cat
        .into_iter()
        .map(|(category, (files, bytes))| CategoryUsage { category, files, bytes })
        .collect();
    categories.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.category.cmp(b.category)));

    let mut folders: Vec<FolderUsage> = by_dir
        .into_iter()
        .map(|(path, (files, bytes))| FolderUsage { path, files, bytes })
        .collect();
    folders.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.path.cmp(&b.path)));
    folders.truncate(TOP);

    // Over-fetch a little: some rows may point at files that have since vanished.
    let largest = stored_files(
        conn,
        &format!("SELECT id, path, size, mtime_ns FROM files ORDER BY size DESC, id LIMIT {}", TOP * 5),
    )?;
    let old = stored_files(
        conn,
        &format!(
            "SELECT id, path, size, mtime_ns FROM files
             WHERE mtime_ns < (unixepoch() - {OLD_SECONDS}) * 1000000000
             ORDER BY size DESC, id LIMIT {}",
            TOP * 5
        ),
    )?;
    Ok(Analytics { categories, largest, folders, old })
}

/// Folders with nothing in them, inside the tracked folders. Looks at the disk (not the
/// database) because empty folders hold no files for the scanner to record.
pub fn empty_folders(conn: &Connection, config: &Config) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    'roots: for (_, root) in crate::db::folder_rows(conn)? {
        if !Path::new(&root).is_dir() {
            continue;
        }
        let walker = WalkDir::new(&root)
            .follow_links(false)
            .min_depth(1)
            .into_iter()
            .filter_entry(|e| !config.is_excluded(e));
        for entry in walker.flatten() {
            if !entry.file_type().is_dir() {
                continue;
            }
            if fs::read_dir(entry.path()).map(|mut d| d.next().is_none()).unwrap_or(false) {
                out.push(entry.path().to_string_lossy().into_owned());
                if out.len() >= MAX_EMPTY {
                    break 'roots;
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::sync::atomic::AtomicBool;

    fn env() -> (Connection, tempfile::TempDir) {
        let conn = crate::db::init(Connection::open_in_memory().unwrap()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::db::add_folder(&conn, dir.path().to_str().unwrap()).unwrap();
        (conn, dir)
    }

    fn write(dir: &Path, name: &str, bytes: usize) {
        let p = dir.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; bytes]).unwrap();
    }

    fn scan(conn: &mut Connection) {
        let id = crate::scan::begin_scan(conn).unwrap();
        crate::scan::run(conn, id, &AtomicBool::new(false), &Config::default(), &mut |_| {}).unwrap();
    }

    #[test]
    fn breaks_down_by_category_folder_and_size() {
        let (mut conn, dir) = env();
        write(dir.path(), "movies/big.mkv", 5000);
        write(dir.path(), "movies/small.mp4", 100);
        write(dir.path(), "docs/a.pdf", 300);
        write(dir.path(), "docs/b.pdf", 200);
        write(dir.path(), "readme.txt", 10); // unsupported: not scanned
        scan(&mut conn);

        let a = analytics(&conn).unwrap();
        assert_eq!(a.categories[0], CategoryUsage { category: "video", files: 2, bytes: 5100 });
        assert_eq!(a.categories[1], CategoryUsage { category: "document", files: 2, bytes: 500 });
        assert_eq!(a.largest[0].size, 5000);
        assert_eq!(a.largest.len(), 4);
        assert!(a.folders[0].path.ends_with("movies"));
        assert_eq!((a.folders[0].files, a.folders[0].bytes), (2, 5100));
        assert!(a.old.is_empty());
    }

    #[test]
    fn old_files_are_listed_biggest_first_and_vanished_ones_are_skipped() {
        let (mut conn, dir) = env();
        write(dir.path(), "a.zip", 100);
        write(dir.path(), "b.zip", 900);
        write(dir.path(), "c.zip", 5000);
        write(dir.path(), "new.zip", 50);
        scan(&mut conn);
        let three_years_ns: i64 = 3 * 365 * 24 * 3600 * 1_000_000_000;
        conn.execute(
            "UPDATE files SET mtime_ns = mtime_ns - ?1 WHERE path NOT LIKE '%new.zip'",
            params![three_years_ns],
        )
        .unwrap();
        fs::remove_file(dir.path().join("c.zip")).unwrap();

        let a = analytics(&conn).unwrap();
        let sizes: Vec<i64> = a.old.iter().map(|f| f.size).collect();
        assert_eq!(sizes, vec![900, 100]);
        assert!(a.largest.iter().all(|f| !f.path.ends_with("c.zip")));
    }

    #[test]
    fn finds_empty_folders_only() {
        let (conn, dir) = env();
        fs::create_dir_all(dir.path().join("empty")).unwrap();
        fs::create_dir_all(dir.path().join("full")).unwrap();
        fs::write(dir.path().join("full/x.txt"), b"x").unwrap();
        fs::create_dir_all(dir.path().join("nested/inner")).unwrap();
        fs::create_dir_all(dir.path().join(".git/objects")).unwrap(); // always excluded

        let found = empty_folders(&conn, &Config::default()).unwrap();
        let names: Vec<&str> = found.iter().map(|p| p.rsplit('/').next().unwrap()).collect();
        assert_eq!(names, vec!["empty", "inner"]);
    }

    #[test]
    fn empty_database_is_fine() {
        let (conn, _dir) = env();
        assert_eq!(analytics(&conn).unwrap(), Analytics::default());
    }
}

//! Review-screen data: image metadata and the recommended copy to keep.

use std::fs;
use std::time::UNIX_EPOCH;

use rusqlite::{params, Connection};

use crate::scan::{DupFile, DupGroup};

/// Fills in dimensions (cached in the DB), creation times and the recommended copy.
pub fn enrich(conn: &Connection, groups: &mut [DupGroup]) -> Result<(), String> {
    let mut save = conn
        .prepare_cached("UPDATE files SET width = ?2, height = ?3 WHERE id = ?1")
        .map_err(|e| e.to_string())?;

    for g in groups.iter_mut() {
        for f in g.files.iter_mut() {
            // Only images have dimensions; leave other file types blank.
            let image = crate::scan::is_image_path(std::path::Path::new(&f.path));
            if image && (f.width.is_none() || f.height.is_none()) {
                // (0, 0) records "unreadable" so we don't retry on every listing.
                let (w, h) = image::image_dimensions(&f.path).unwrap_or((0, 0));
                save.execute(params![f.id, w, h]).map_err(|e| e.to_string())?;
                f.width = Some(w);
                f.height = Some(h);
            }
            f.created = fs::metadata(&f.path)
                .and_then(|m| m.created())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
        }
        g.keep_id = recommend(&g.files);
    }
    Ok(())
}

/// Like `enrich`, for similar groups: also works out the bytes each group would free
/// (which depends on the recommended copy) and puts the biggest savings first.
pub fn enrich_similar(conn: &Connection, groups: &mut Vec<DupGroup>) -> Result<(), String> {
    enrich(conn, groups)?;
    for g in groups.iter_mut() {
        g.wasted = crate::similar::wasted_bytes(&g.files, g.keep_id);
    }
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted).then(a.sha256.cmp(&b.sha256)));
    Ok(())
}

fn pixels(f: &DupFile) -> u64 {
    f.width.unwrap_or(0) as u64 * f.height.unwrap_or(0) as u64
}

/// Earliest known timestamp: copies often get a fresh creation time, so a modified
/// time that predates it is the better signal of age.
fn age(f: &DupFile) -> i64 {
    f.created.map_or(f.modified, |c| c.min(f.modified))
}

/// Best copy to keep. Among the near-highest-resolution copies (at least 75% of the largest
/// pixel count), the sharpest wins when sharpness is known; then highest resolution, then
/// oldest, then path (for a deterministic result). Exact copies share resolution and have no
/// sharpness, so for them this is simply "highest resolution, then oldest".
pub fn recommend(files: &[DupFile]) -> i64 {
    let max_px = files.iter().map(pixels).max().unwrap_or(0);
    let top_tier = |f: &DupFile| pixels(f) * 4 >= max_px * 3;
    files
        .iter()
        .min_by(|a, b| {
            top_tier(b)
                .cmp(&top_tier(a))
                .then_with(|| {
                    let (sa, sb) = (a.sharpness.unwrap_or(-1.0), b.sharpness.unwrap_or(-1.0));
                    sb.total_cmp(&sa)
                })
                .then(pixels(b).cmp(&pixels(a)))
                .then(age(a).cmp(&age(b)))
                .then(a.path.cmp(&b.path))
        })
        .map(|f| f.id)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(id: i64, path: &str, w: u32, h: u32, modified: i64, created: Option<i64>) -> DupFile {
        DupFile {
            id,
            path: path.into(),
            size: 1,
            file_id: None,
            modified,
            created,
            width: Some(w),
            height: Some(h),
            sharpness: None,
        }
    }

    #[test]
    fn prefers_highest_resolution() {
        let files = [f(1, "a", 100, 100, 1, None), f(2, "b", 200, 200, 99, None)];
        assert_eq!(recommend(&files), 2);
    }

    fn sharp(id: i64, w: u32, h: u32, sharpness: f64) -> DupFile {
        DupFile { sharpness: Some(sharpness), ..f(id, &format!("p{id}"), w, h, 1, None) }
    }

    #[test]
    fn prefers_the_sharpest_among_high_resolution_copies() {
        // Same-ish resolution: the sharper one wins even with fewer pixels (within 75%).
        let files = [sharp(1, 100, 100, 5.0), sharp(2, 90, 90, 50.0)];
        assert_eq!(recommend(&files), 2);
    }

    #[test]
    fn a_much_smaller_image_never_wins_on_sharpness() {
        let files = [sharp(1, 100, 100, 5.0), sharp(2, 40, 40, 500.0)];
        assert_eq!(recommend(&files), 1);
    }

    #[test]
    fn known_sharpness_beats_unknown_within_the_top_tier() {
        let files = [f(1, "a", 100, 100, 1, None), sharp(2, 100, 100, 1.0)];
        assert_eq!(recommend(&files), 2);
    }

    #[test]
    fn then_prefers_oldest() {
        let files = [f(1, "a", 100, 100, 50, None), f(2, "b", 100, 100, 10, None)];
        assert_eq!(recommend(&files), 2);
    }

    #[test]
    fn age_uses_earliest_of_created_and_modified() {
        // 1 was created long ago (5) even though modified recently; 2 is newer overall.
        let files = [f(1, "a", 10, 10, 90, Some(5)), f(2, "b", 10, 10, 20, Some(30))];
        assert_eq!(recommend(&files), 1);
    }

    #[test]
    fn then_path_is_deterministic_tiebreak() {
        let files = [f(1, "b", 10, 10, 5, None), f(2, "a", 10, 10, 5, None)];
        assert_eq!(recommend(&files), 2);
    }

    #[test]
    fn unreadable_dimensions_lose_to_known_ones() {
        let files = [f(1, "a", 0, 0, 1, None), f(2, "b", 10, 10, 99, None)];
        assert_eq!(recommend(&files), 2);
    }

    #[test]
    fn enrich_reads_and_caches_dimensions() {
        let conn = crate::db::init(Connection::open_in_memory().unwrap()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let folder = crate::db::add_folder(&conn, dir.path().to_str().unwrap()).unwrap();
        let mut ids = Vec::new();
        for name in ["a.png", "b.png"] {
            let p = dir.path().join(name);
            image::RgbImage::new(7, 3).save(&p).unwrap();
            conn.execute(
                "INSERT INTO files (folder_id, path, size, mtime_ns, sha256, last_seen_scan)
                 VALUES (?1, ?2, 1, 1, 'h', 1)",
                params![folder.id, p.to_str().unwrap()],
            )
            .unwrap();
            ids.push(conn.last_insert_rowid());
        }
        let mut groups = crate::scan::duplicate_groups(&conn).unwrap();
        assert_eq!(groups.len(), 1);
        enrich(&conn, &mut groups).unwrap();
        assert_eq!(groups[0].files[0].width, Some(7));
        assert_eq!(groups[0].files[0].height, Some(3));
        assert!(groups[0].keep_id > 0);
        let w: Option<u32> = conn
            .query_row("SELECT width FROM files WHERE id = ?1", params![ids[0]], |r| r.get(0))
            .unwrap();
        assert_eq!(w, Some(7), "dimensions are cached in the database");
    }
}

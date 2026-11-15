//! On-disk thumbnail cache, served to the webview through the `thumb://` scheme.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

use image::ImageReader;
use rusqlite::{params, Connection, OptionalExtension};

const MAX_EDGE: u32 = 256;
const MAX_CONCURRENT: usize = 4;

/// Limits how many thumbnails decode at once so a screen full of images
/// can't spawn dozens of memory-hungry decoders.
struct Gate {
    count: Mutex<usize>,
    cv: Condvar,
}
static GATE: Gate = Gate { count: Mutex::new(0), cv: Condvar::new() };

struct Permit;
impl Permit {
    fn acquire() -> Permit {
        let mut n = GATE.count.lock().unwrap_or_else(|e| e.into_inner());
        while *n >= MAX_CONCURRENT {
            n = GATE.cv.wait(n).unwrap_or_else(|e| e.into_inner());
        }
        *n += 1;
        Permit
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        *GATE.count.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
        GATE.cv.notify_one();
    }
}

pub struct Thumb {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

fn mime_for(path: &Path) -> &'static str {
    if path.extension().is_some_and(|e| e == "png") { "image/png" } else { "image/jpeg" }
}

/// Resizes `src` into the cache at `<cache_dir>/<key>.{jpg,png}` (PNG only when alpha is present).
fn generate(src: &str, cache_dir: &Path, key: &str) -> Result<PathBuf, String> {
    let img = ImageReader::open(src)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let thumb = img.thumbnail(MAX_EDGE, MAX_EDGE);

    let (ext, out) = if thumb.color().has_alpha() {
        ("png", thumb)
    } else {
        ("jpg", image::DynamicImage::ImageRgb8(thumb.to_rgb8()))
    };
    let dest = cache_dir.join(format!("{key}.{ext}"));
    // Write then rename so a concurrent reader never sees a half-written file.
    let tmp = cache_dir.join(format!("{key}.{}.tmp", std::process::id()));
    out.save_with_format(
        &tmp,
        if ext == "png" { image::ImageFormat::Png } else { image::ImageFormat::Jpeg },
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Returns the thumbnail for a tracked file id, generating it on first request.
/// Only files recorded in the database can be served.
pub fn thumbnail(conn: &Mutex<Connection>, cache_dir: &Path, id: i64) -> Result<Thumb, String> {
    let (path, sha, mtime): (String, Option<String>, i64) = {
        let c = conn.lock().map_err(|_| "database lock poisoned".to_string())?;
        c.query_row("SELECT path, sha256, mtime_ns FROM files WHERE id = ?1", params![id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("unknown file")?
    };
    // Identical files share a thumbnail via their content hash.
    let key = sha.unwrap_or_else(|| format!("f{id}_{mtime}"));

    for ext in ["jpg", "png"] {
        let cached = cache_dir.join(format!("{key}.{ext}"));
        if let Ok(bytes) = fs::read(&cached) {
            return Ok(Thumb { bytes, mime: mime_for(&cached) });
        }
    }

    let _permit = Permit::acquire();
    fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    let dest = generate(&path, cache_dir, &key)?;
    Ok(Thumb { bytes: fs::read(&dest).map_err(|e| e.to_string())?, mime: mime_for(&dest) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(name: &str, save: impl FnOnce(&Path)) -> (Mutex<Connection>, tempfile::TempDir, i64) {
        let conn = crate::db::init(Connection::open_in_memory().unwrap()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let folder = crate::db::add_folder(&conn, dir.path().to_str().unwrap()).unwrap();
        let p = dir.path().join(name);
        save(&p);
        conn.execute(
            "INSERT INTO files (folder_id, path, size, mtime_ns, sha256, last_seen_scan)
             VALUES (?1, ?2, 1, 1, 'abc', 1)",
            params![folder.id, p.to_str().unwrap()],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        (Mutex::new(conn), dir, id)
    }

    #[test]
    fn makes_downscaled_jpeg_and_caches_it() {
        let (conn, dir, id) = setup("big.png", |p| image::RgbImage::new(1000, 500).save(p).unwrap());
        let cache = dir.path().join("cache");
        let t = thumbnail(&conn, &cache, id).unwrap();
        assert_eq!(t.mime, "image/jpeg");
        let img = image::load_from_memory(&t.bytes).unwrap();
        assert_eq!((img.width(), img.height()), (256, 128));
        assert!(cache.join("abc.jpg").exists());
        assert!(thumbnail(&conn, &cache, id).is_ok(), "served from cache");
    }

    #[test]
    fn alpha_images_stay_png() {
        let (conn, dir, id) = setup("a.png", |p| image::RgbaImage::new(10, 10).save(p).unwrap());
        let t = thumbnail(&conn, &dir.path().join("cache"), id).unwrap();
        assert_eq!(t.mime, "image/png");
    }

    #[test]
    fn corrupt_file_errors_and_unknown_id_rejected() {
        let (conn, dir, id) = setup("bad.jpg", |p| fs::write(p, b"not an image").unwrap());
        let cache = dir.path().join("cache");
        assert!(thumbnail(&conn, &cache, id).is_err());
        assert!(thumbnail(&conn, &cache, 9999).is_err());
    }
}

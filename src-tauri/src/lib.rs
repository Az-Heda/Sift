mod db;
mod review;
mod scan;
mod thumbs;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct Db(Mutex<Connection>);

impl Db {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        let conn = self.0.lock().map_err(|_| "database lock poisoned".to_string())?;
        f(&conn)
    }
}

/// Location of the database file, so the scan thread can open its own connection
/// instead of holding the UI's lock for the length of a scan.
pub struct DbPath(PathBuf);

#[derive(Default)]
struct Running {
    cancel: Arc<AtomicBool>,
    progress: Option<scan::Progress>,
}

#[derive(Default)]
pub struct ScanState(Mutex<Option<Running>>);

#[derive(Serialize)]
struct ScanStatus {
    running: bool,
    progress: Option<scan::Progress>,
}

/// Clears the running-scan slot when the scan thread ends, even on panic.
struct ClearOnDrop(AppHandle);
impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        if let Ok(mut g) = self.0.state::<ScanState>().0.lock() {
            *g = None;
        }
    }
}

#[tauri::command]
fn start_scan(app: AppHandle, state: State<'_, ScanState>, path: State<'_, DbPath>) -> Result<(), String> {
    let cancel = {
        let mut g = state.0.lock().map_err(|_| "scan state poisoned".to_string())?;
        if g.is_some() {
            return Err("A scan is already running".into());
        }
        let running = Running::default();
        let cancel = running.cancel.clone();
        *g = Some(running);
        cancel
    };

    let db_path = path.0.clone();
    std::thread::spawn(move || {
        let _guard = ClearOnDrop(app.clone());
        let result = (|| {
            let mut conn = db::open(&db_path)?;
            let config = scan::Config::load(&conn)?;
            let scan_id = scan::begin_scan(&conn)?;
            let mut last_emit = Instant::now() - Duration::from_secs(1);
            let handle = app.clone();
            scan::run(&mut conn, scan_id, &cancel, &config, &mut |p| {
                if let Ok(mut g) = handle.state::<ScanState>().0.lock() {
                    if let Some(r) = g.as_mut() {
                        r.progress = Some(p.clone());
                    }
                }
                // ~10 events/second; phase changes always go through.
                if last_emit.elapsed() >= Duration::from_millis(100) || p.phase != "discovering" && p.hash_done == 0 {
                    last_emit = Instant::now();
                    let _ = handle.emit("scan-progress", p);
                }
            })
        })();
        match result {
            Ok(summary) => {
                let _ = app.emit("scan-finished", summary);
            }
            Err(e) => {
                let _ = app.emit("scan-error", e);
            }
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_scan(state: State<'_, ScanState>) -> Result<(), String> {
    if let Some(r) = state.0.lock().map_err(|_| "scan state poisoned".to_string())?.as_ref() {
        r.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
fn scan_status(state: State<'_, ScanState>) -> Result<ScanStatus, String> {
    let g = state.0.lock().map_err(|_| "scan state poisoned".to_string())?;
    Ok(ScanStatus {
        running: g.is_some(),
        progress: g.as_ref().and_then(|r| r.progress.clone()),
    })
}

#[tauri::command]
async fn list_duplicates(db: State<'_, Db>) -> Result<Vec<scan::DupGroup>, String> {
    db.with(|c| {
        let mut groups = scan::duplicate_groups(c)?;
        review::enrich(c, &mut groups)?;
        Ok(groups)
    })
}

#[tauri::command]
async fn get_stats(db: State<'_, Db>) -> Result<scan::Stats, String> {
    db.with(scan::stats)
}

#[tauri::command]
async fn get_insights(db: State<'_, Db>) -> Result<scan::Insights, String> {
    db.with(scan::insights)
}

fn tracked_path(db: &Db, id: i64) -> Result<String, String> {
    db.with(|c| {
        c.query_row("SELECT path FROM files WHERE id = ?1", [id], |r| r.get(0))
            .map_err(|_| "unknown file".to_string())
    })
}

/// Shows the file selected in the OS file manager. Only tracked files are accepted.
#[tauri::command]
async fn reveal_file(db: State<'_, Db>, id: i64) -> Result<(), String> {
    let path = tracked_path(&db, id)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
}

/// Opens the folder that contains a tracked file.
#[tauri::command]
async fn open_containing_folder(db: State<'_, Db>, id: i64) -> Result<(), String> {
    let path = tracked_path(&db, id)?;
    let dir = std::path::Path::new(&path).parent().ok_or("file has no parent folder")?;
    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_folders(db: State<'_, Db>) -> Result<Vec<db::Folder>, String> {
    db.with(db::list_folders)
}

#[tauri::command]
async fn add_folder(db: State<'_, Db>, path: String) -> Result<db::Folder, String> {
    db.with(|c| db::add_folder(c, &path))
}

#[tauri::command]
async fn remove_folder(db: State<'_, Db>, id: i64) -> Result<(), String> {
    db.with(|c| db::remove_folder(c, id))
}

#[tauri::command]
async fn get_setting(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    db.with(|c| db::get_setting(c, &key))
}

#[tauri::command]
async fn set_setting(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    db.with(|c| db::set_setting(c, &key, &value))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("thumb", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            // `/<id>` is a thumbnail, `/full-<id>` the original image. (No slash in the
            // second form: the frontend percent-encodes the path it hands to the scheme.)
            let path = request.uri().path().trim_start_matches('/').to_owned();
            let (id_part, full) = match path.strip_prefix("full-") {
                Some(p) => (p.to_owned(), true),
                None => (path, false),
            };
            let id = id_part.parse::<i64>();
            // Decoding is CPU/IO heavy: keep it off the main thread.
            tauri::async_runtime::spawn_blocking(move || {
                let result = id.map_err(|e| e.to_string()).and_then(|id| {
                    if full {
                        return thumbs::full_image(&app.state::<Db>().0, id);
                    }
                    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("thumbs");
                    thumbs::thumbnail(&app.state::<Db>().0, &cache, id)
                });
                let response = match result {
                    Ok(t) => tauri::http::Response::builder()
                        .header("Content-Type", t.mime)
                        .header("Cache-Control", "max-age=3600")
                        .body(t.bytes),
                    Err(_) => tauri::http::Response::builder().status(404).body(Vec::new()),
                };
                if let Ok(r) = response {
                    responder.respond(r);
                }
            });
        })
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("sift.db");
            let conn = db::open(&db_path)?;
            app.manage(Db(Mutex::new(conn)));
            app.manage(DbPath(db_path));
            app.manage(ScanState::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_folders,
            add_folder,
            remove_folder,
            get_setting,
            set_setting,
            start_scan,
            cancel_scan,
            scan_status,
            list_duplicates,
            get_stats,
            get_insights,
            reveal_file,
            open_containing_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sift");
}

mod cleanup;
mod db;
mod monitor;
mod review;
mod scan;
mod similar;
mod storage;
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

/// The live folder watcher, if monitoring is on. Dropping it stops monitoring.
#[derive(Default)]
pub struct MonitorState(Mutex<Option<notify::RecommendedWatcher>>);

/// (Re)builds the folder watcher from the current settings and folders. Call it whenever
/// something that decides what is watched changes.
fn refresh_monitor(app: &AppHandle) {
    let build = || -> Result<Option<notify::RecommendedWatcher>, String> {
        let db = app.state::<Db>();
        let (folders, ignored) = db.with(|c| {
            if !monitor::enabled(c)? {
                return Ok((Vec::new(), Vec::new()));
            }
            let folders: Vec<PathBuf> = db::folder_rows(c)?.into_iter().map(|(_, p)| p.into()).collect();
            let mut config = scan::Config::load(c)?;
            config.excluded_paths.push(cleanup::dir(c, &default_quarantine_dir(app)?)?);
            Ok((folders, config.excluded_paths))
        })?;
        if folders.is_empty() {
            return Ok(None);
        }
        monitor::start(app, &folders, ignored).map(Some)
    };
    let watcher = build().unwrap_or_else(|e| {
        eprintln!("folder monitoring unavailable: {e}");
        None
    });
    if let Ok(mut g) = app.state::<MonitorState>().0.lock() {
        *g = watcher; // replaces (and so stops) any previous watcher
    }
}

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

/// Message of the error `launch_scan` returns while another scan is in flight.
pub const SCAN_BUSY: &str = "A scan is already running";

/// Starts a background scan. Used by the Scan button and by folder monitoring.
pub fn launch_scan(app: &AppHandle) -> Result<(), String> {
    let cancel = {
        let state = app.state::<ScanState>();
        let mut g = state.0.lock().map_err(|_| "scan state poisoned".to_string())?;
        if g.is_some() {
            return Err(SCAN_BUSY.into());
        }
        let running = Running::default();
        let cancel = running.cancel.clone();
        *g = Some(running);
        cancel
    };
    let _ = app.emit("scan-started", ());

    let db_path = app.state::<DbPath>().0.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let _guard = ClearOnDrop(app.clone());
        let result = (|| {
            let mut conn = db::open(&db_path)?;
            let mut config = scan::Config::load(&conn)?;
            // Never scan quarantined files, wherever the user put the folder.
            config.excluded_paths.push(cleanup::dir(&conn, &default_quarantine_dir(&app)?)?);
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
fn start_scan(app: AppHandle) -> Result<(), String> {
    launch_scan(&app)
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
async fn list_similar(db: State<'_, Db>) -> Result<Vec<scan::DupGroup>, String> {
    db.with(|c| {
        let mut groups = similar::similar_groups(c)?;
        review::enrich_similar(c, &mut groups)?;
        Ok(groups)
    })
}

#[tauri::command]
async fn get_storage(db: State<'_, Db>) -> Result<storage::Analytics, String> {
    db.with(storage::analytics)
}

/// Walks the disk, so it runs off the async worker and is requested separately.
#[tauri::command]
async fn get_empty_folders(app: AppHandle) -> Result<Vec<String>, String> {
    let default = default_quarantine_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let db = &app.state::<Db>();
        db.with(|c| {
            let mut config = scan::Config::load(c)?;
            config.excluded_paths.push(cleanup::dir(c, &default)?);
            storage::empty_folders(c, &config)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_stats(db: State<'_, Db>) -> Result<scan::Stats, String> {
    db.with(scan::stats)
}

/// Cleanup rewrites the files table and moves files the scanner is reading, so it waits.
fn ensure_idle(state: &ScanState) -> Result<(), String> {
    let busy = state.0.lock().map_err(|_| "scan state poisoned".to_string())?.is_some();
    if busy { Err("Wait for the running scan to finish first".into()) } else { Ok(()) }
}

fn default_quarantine_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("quarantine"))
}

#[derive(Serialize)]
struct QuarantineDir {
    path: PathBuf,
    default: PathBuf,
}

#[tauri::command]
async fn get_quarantine_dir(app: AppHandle, db: State<'_, Db>) -> Result<QuarantineDir, String> {
    let default = default_quarantine_dir(&app)?;
    db.with(|c| Ok(QuarantineDir { path: cleanup::dir(c, &default)?, default: default.clone() }))
}

/// An empty path restores the default location.
#[tauri::command]
async fn set_quarantine_dir(app: AppHandle, db: State<'_, Db>, path: String) -> Result<QuarantineDir, String> {
    let default = default_quarantine_dir(&app)?;
    let dir = db.with(|c| Ok(QuarantineDir { path: cleanup::set_dir(c, &default, &path)?, default: default.clone() }))?;
    refresh_monitor(&app); // the new quarantine folder must not trigger rescans
    Ok(dir)
}

#[tauri::command]
async fn quarantine_files(
    app: AppHandle,
    db: State<'_, Db>,
    scan: State<'_, ScanState>,
    ids: Vec<i64>,
    mode: Option<String>,
) -> Result<cleanup::CleanupResult, String> {
    ensure_idle(&scan)?;
    let mode = cleanup::Mode::parse(mode.as_deref())?;
    let default = default_quarantine_dir(&app)?;
    db.with(|c| cleanup::quarantine(c, &cleanup::dir(c, &default)?, &ids, mode))
}

#[tauri::command]
async fn delete_files(
    db: State<'_, Db>,
    scan: State<'_, ScanState>,
    ids: Vec<i64>,
    mode: Option<String>,
) -> Result<cleanup::CleanupResult, String> {
    ensure_idle(&scan)?;
    let mode = cleanup::Mode::parse(mode.as_deref())?;
    db.with(|c| cleanup::delete_files(c, &ids, mode))
}

#[tauri::command]
async fn list_quarantine(db: State<'_, Db>) -> Result<Vec<cleanup::QuarantineItem>, String> {
    db.with(cleanup::list_quarantine)
}

#[tauri::command]
async fn restore_quarantined(
    db: State<'_, Db>,
    scan: State<'_, ScanState>,
    ids: Vec<i64>,
) -> Result<cleanup::CleanupResult, String> {
    ensure_idle(&scan)?;
    db.with(|c| cleanup::restore(c, &ids))
}

#[tauri::command]
async fn delete_quarantined(
    db: State<'_, Db>,
    scan: State<'_, ScanState>,
    ids: Vec<i64>,
) -> Result<cleanup::CleanupResult, String> {
    ensure_idle(&scan)?;
    db.with(|c| cleanup::delete_quarantined(c, &ids))
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
async fn add_folder(app: AppHandle, db: State<'_, Db>, path: String) -> Result<db::Folder, String> {
    let folder = db.with(|c| db::add_folder(c, &path))?;
    refresh_monitor(&app);
    Ok(folder)
}

#[tauri::command]
async fn remove_folder(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    db.with(|c| db::remove_folder(c, id))?;
    refresh_monitor(&app);
    Ok(())
}

#[tauri::command]
async fn get_setting(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    db.with(|c| db::get_setting(c, &key))
}

#[tauri::command]
async fn set_setting(app: AppHandle, db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    db.with(|c| db::set_setting(c, &key, &value))?;
    if key == monitor::SETTING_KEY || key == "scan.excluded_paths" {
        refresh_monitor(&app);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("thumb", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            // `/<id>` thumbnail, `/full-<id>` original image, `/q-<id>` thumbnail of a quarantined
            // item. No slashes inside: the frontend percent-encodes the path it gives the scheme.
            let path = request.uri().path().trim_start_matches('/').to_owned();
            #[derive(Clone, Copy)]
            enum Kind { Thumb, Full, Quarantined }
            let (kind, id) = if let Some(p) = path.strip_prefix("full-") {
                (Kind::Full, p.parse::<i64>())
            } else if let Some(p) = path.strip_prefix("q-") {
                (Kind::Quarantined, p.parse::<i64>())
            } else {
                (Kind::Thumb, path.parse::<i64>())
            };
            // Decoding is CPU/IO heavy: keep it off the main thread.
            tauri::async_runtime::spawn_blocking(move || {
                let result = id.map_err(|e| e.to_string()).and_then(|id| {
                    let db = &app.state::<Db>().0;
                    if let Kind::Full = kind {
                        return thumbs::full_image(db, id);
                    }
                    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("thumbs");
                    match kind {
                        Kind::Quarantined => thumbs::quarantine_thumbnail(db, &cache, id),
                        _ => thumbs::thumbnail(db, &cache, id),
                    }
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
            app.manage(MonitorState::default());
            refresh_monitor(app.handle());
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
            list_similar,
            get_storage,
            get_empty_folders,
            get_stats,
            get_insights,
            quarantine_files,
            delete_files,
            list_quarantine,
            restore_quarantined,
            delete_quarantined,
            get_quarantine_dir,
            set_quarantine_dir,
            reveal_file,
            open_containing_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sift");
}

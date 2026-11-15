mod db;

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{Manager, State};

pub struct Db(Mutex<Connection>);

impl Db {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        let conn = self.0.lock().map_err(|_| "database lock poisoned".to_string())?;
        f(&conn)
    }
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
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = db::open(&dir.join("sift.db"))?;
            app.manage(Db(Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_folders,
            add_folder,
            remove_folder,
            get_setting,
            set_setting
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sift");
}

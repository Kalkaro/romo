use crate::model::{Board, View};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub format: String,
    pub version: u32,
    pub board: Board,
    pub view: View,
}

impl Document {
    pub fn new(board: Board, view: View) -> Self {
        Self {
            format: "romo".into(),
            version: 1,
            board,
            view,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != "romo" || self.version != 1 {
            return Err("This is not a supported Romo board (expected version 1).".into());
        }
        self.board.validate()?;
        if !self.view.pan.is_finite()
            || self.view.pan.x.abs() > 10_000_000.0
            || self.view.pan.y.abs() > 10_000_000.0
            || !self.view.zoom.is_finite()
            || !(0.15..=3.0).contains(&self.view.zoom)
        {
            return Err("The board contains an invalid canvas view.".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub document: Document,
    pub path: Option<PathBuf>,
    pub dirty: bool,
}

/// A board and the session around it, copied so they can be written off the
/// main thread.
pub struct Snapshot {
    /// Snapshots are numbered in the order they are taken.
    pub number: u64,
    pub document: Document,
    /// The board's file in the library, when there is a library.
    pub board: Option<PathBuf>,
    pub session: Option<PathBuf>,
    /// The board path and unsaved state the session restores.
    pub path: Option<PathBuf>,
    pub dirty: bool,
}

impl Snapshot {
    /// Write the board, then the session. Writes take turns through `latest`,
    /// the number of the newest snapshot written, so a snapshot whose turn
    /// comes after a newer one is dropped instead of replacing newer files.
    pub fn write(self, latest: &Mutex<u64>) -> Result<(), String> {
        let mut latest = latest.lock().unwrap_or_else(PoisonError::into_inner);
        if *latest > self.number {
            return Ok(());
        }
        *latest = self.number;
        if let Some(path) = &self.board {
            save(path, &self.document)?;
        }
        let path = self.session.ok_or_else(|| {
            "Cannot find a local data folder. Save your board to a file.".to_owned()
        })?;
        save_session(
            &path,
            &Session {
                document: self.document,
                path: self.path,
                dirty: self.dirty,
            },
        )
    }
}

pub fn session_path() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("session.json"))
}

pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ROMO_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    directories::ProjectDirs::from("app", "romo", "Romo")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
}

#[derive(Clone, Debug)]
pub struct BoardEntry {
    pub path: PathBuf,
    pub title: String,
}

pub fn board_path(folder: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    folder.join(format!("board-{stamp}-{}.romo", std::process::id()))
}

pub fn library(folder: &Path) -> Vec<BoardEntry> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "romo"))
        .collect();
    entries
        .sort_by_key(|entry| std::cmp::Reverse(entry.metadata().and_then(|m| m.modified()).ok()));
    #[derive(Deserialize)]
    struct Summary {
        format: String,
        version: u32,
        board: Title,
    }
    #[derive(Deserialize)]
    struct Title {
        title: String,
    }
    entries
        .into_iter()
        .filter_map(|entry| {
            let summary: Summary =
                serde_json::from_slice(&read_limited(&entry.path()).ok()?).ok()?;
            if summary.format != "romo" || summary.version != 1 {
                return None;
            }
            Some(BoardEntry {
                path: entry.path(),
                title: summary.board.title,
            })
        })
        .collect()
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub snapping: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self { snapping: true }
    }
}
pub fn load_settings(path: &Path) -> Settings {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
pub fn save_settings(path: &Path, settings: &Settings) -> Result<(), String> {
    atomic_write(
        path,
        &serde_json::to_vec(settings).map_err(|e| e.to_string())?,
    )
}

pub fn load(path: &Path) -> Result<Document, String> {
    let document: Document = serde_json::from_slice(&read_limited(path)?)
        .map_err(|e| format!("Cannot read this board: {e}"))?;
    document.validate()?;
    Ok(document)
}

pub fn load_session(path: &Path) -> Result<Session, String> {
    let session: Session = serde_json::from_slice(&read_limited(path)?)
        .map_err(|e| format!("Cannot restore the previous session: {e}"))?;
    session.document.validate()?;
    Ok(session)
}

fn read_limited(path: &Path) -> Result<Vec<u8>, String> {
    const LIMIT: u64 = 32 * 1024 * 1024;
    let file = fs::File::open(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read board: {e}"))?;
    if bytes.len() as u64 > LIMIT {
        return Err("The board file is larger than 32 MB.".into());
    }
    Ok(bytes)
}

pub fn save(path: &Path, document: &Document) -> Result<(), String> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document).map_err(|e| e.to_string())?;
    atomic_write(path, &bytes)
}

pub fn save_session(path: &Path, session: &Session) -> Result<(), String> {
    session.document.validate()?;
    let bytes = serde_json::to_vec(session).map_err(|e| e.to_string())?;
    atomic_write(path, &bytes)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("Board exceeds the 32 MB storage limit.".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create the save folder: {e}"))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temporary = parent.join(format!(".romo-{}-{stamp}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok::<_, std::io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Cannot save {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_board_can_be_reopened_and_overwritten_without_losing_content() {
        let folder = std::env::temp_dir().join(format!("romo-test-{}", std::process::id()));
        let path = folder.join("board.romo");
        let mut document = Document::new(Board::welcome(), View::default());
        save(&path, &document).unwrap();
        assert_eq!(load(&path).unwrap().board, document.board);
        document.board.title = "Updated board".into();
        save(&path, &document).unwrap();
        assert_eq!(load(&path).unwrap().board.title, "Updated board");
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn an_older_snapshot_written_late_keeps_the_newer_save() {
        let folder = std::env::temp_dir().join(format!("romo-snapshot-{}", std::process::id()));
        let (board_path, session_path) = (folder.join("board.romo"), folder.join("session.json"));
        let snapshot = |number, title: &str| {
            let board = Board {
                title: title.into(),
                ..Board::default()
            };
            Snapshot {
                number,
                document: Document::new(board, View::default()),
                board: Some(board_path.clone()),
                session: Some(session_path.clone()),
                path: Some(board_path.clone()),
                dirty: false,
            }
        };
        let latest = Mutex::new(0);
        snapshot(2, "Newer").write(&latest).unwrap();
        snapshot(1, "Older").write(&latest).unwrap();
        assert_eq!(load(&board_path).unwrap().board.title, "Newer");
        let session = load_session(&session_path).unwrap();
        assert_eq!(session.document.board.title, "Newer");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn unsupported_files_and_invalid_views_are_rejected() {
        let mut document = Document::new(Board::default(), View::default());
        document.version = 42;
        assert!(document.validate().is_err());
        document.version = 1;
        document.view.zoom = 0.0;
        assert!(document.validate().is_err());
    }

    #[test]
    fn library_keeps_boards_separate_and_preferences_survive_restart() {
        let folder =
            std::env::temp_dir().join(format!("romo-library-check-{}", std::process::id()));
        let boards = folder.join("boards");
        let first = board_path(&boards);
        let mut doc = Document::new(Board::welcome(), View::default());
        doc.board.title = "First board".into();
        save(&first, &doc).unwrap();
        let second = board_path(&boards);
        doc.board.title = "Second board".into();
        save(&second, &doc).unwrap();
        assert_ne!(first, second);
        let entries = library(&boards);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.title == "First board"));
        assert_eq!(load(&first).unwrap().board.title, "First board");
        let prefs = folder.join("settings.json");
        assert!(load_settings(&prefs).snapping);
        save_settings(&prefs, &Settings { snapping: false }).unwrap();
        assert!(!load_settings(&prefs).snapping);
        fs::remove_dir_all(folder).unwrap();
    }
}

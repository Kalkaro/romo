use crate::model::{Board, View};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
};

/// Boards are saved as compressed JSON, with each pen stroke's points as one
/// flat list of coordinates. Version 1 boards, plain JSON with an object per
/// point, still open.
const VERSION: u32 = 2;
/// The largest board file Romo reads or writes.
const FILE_LIMIT: usize = 32 * 1024 * 1024;
/// The largest board unpacked, so a small file can't unpack without end.
const CONTENT_LIMIT: usize = 256 * 1024 * 1024;

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
            version: VERSION,
            board,
            view,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != "romo" || !(1..=VERSION).contains(&self.version) {
            return Err("This is not a supported Romo board (expected version 1 or 2).".into());
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
    /// The board itself, kept here only when there is no library to save it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<Document>,
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
        // A board saved in the library is only named by the session.
        let document = match &self.board {
            Some(path) => {
                save(path, &self.document)?;
                None
            }
            None => Some(self.document),
        };
        let path = self.session.ok_or_else(|| {
            "Cannot find a local data folder. Save your board to a file.".to_owned()
        })?;
        save_session(
            &path,
            &Session {
                document,
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
            if summary.format != "romo" || !(1..=VERSION).contains(&summary.version) {
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

/// The previous session and its board, read from the library unless the
/// session holds it.
pub fn load_session(path: &Path) -> Result<(Session, Document), String> {
    let mut session: Session = serde_json::from_slice(&read_limited(path)?)
        .map_err(|e| format!("Cannot restore the previous session: {e}"))?;
    let document = match session.document.take() {
        Some(document) => {
            document.validate()?;
            document
        }
        None => load(
            session
                .path
                .as_deref()
                .ok_or("The previous session names no board.")?,
        )?,
    };
    Ok((session, document))
}

/// The JSON in a file, unpacked when it is compressed.
fn read_limited(path: &Path) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read board: {e}"))?;
    if bytes.len() > FILE_LIMIT {
        return Err("The board file is larger than 32 MB.".into());
    }
    // Gzip's signature; JSON never starts with it.
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Ok(bytes);
    }
    let mut json = Vec::new();
    GzDecoder::new(bytes.as_slice())
        .take(CONTENT_LIMIT as u64 + 1)
        .read_to_end(&mut json)
        .map_err(|e| format!("Cannot unpack this board: {e}"))?;
    if json.len() > CONTENT_LIMIT {
        return Err("The board is larger than 256 MB unpacked.".into());
    }
    Ok(json)
}

pub fn save(path: &Path, document: &Document) -> Result<(), String> {
    document.validate()?;
    let json = serde_json::to_vec(document).map_err(|e| e.to_string())?;
    if json.len() > CONTENT_LIMIT {
        return Err("Board exceeds the 256 MB storage limit.".into());
    }
    let mut packed = GzEncoder::new(Vec::with_capacity(json.len() / 3), Compression::default());
    let bytes = packed
        .write_all(&json)
        .and_then(|()| packed.finish())
        .map_err(|e| e.to_string())?;
    atomic_write(path, &bytes)
}

pub fn save_session(path: &Path, session: &Session) -> Result<(), String> {
    if let Some(document) = &session.document {
        document.validate()?;
    }
    let bytes = serde_json::to_vec(session).map_err(|e| e.to_string())?;
    atomic_write(path, &bytes)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > FILE_LIMIT {
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
        let (session, restored) = load_session(&session_path).unwrap();
        assert_eq!(restored.board.title, "Newer");
        // The session names the library's board instead of keeping a copy.
        assert!(session.document.is_none());
        assert!(fs::metadata(&session_path).unwrap().len() < 1024);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn boards_save_compressed_and_version_1_files_still_open() {
        let folder = std::env::temp_dir().join(format!("romo-format-{}", std::process::id()));
        let path = folder.join("board.romo");
        let mut board = Board::welcome();
        let id = board.add(
            crate::model::Kind::Pen,
            crate::geometry::pos2(5.0, 5.0),
            crate::model::PALETTE[7],
        );
        board.get_mut(id).unwrap().points = (0..500)
            .map(|i| crate::geometry::pos2(i as f32 * 0.37, (i as f32 * 0.11).sin() * 9.0))
            .collect();
        let document = Document::new(board, View::default());
        save(&path, &document).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(
            bytes.starts_with(&[0x1f, 0x8b]),
            "boards are saved compressed"
        );
        let reopened = load(&path).unwrap();
        assert_eq!(reopened.board, document.board);
        assert_eq!(reopened.version, 2);
        // Version 1: indented JSON, with an object for every point.
        let mut old = serde_json::to_value(&document).unwrap();
        old["version"] = 1.into();
        for item in old["board"]["items"].as_array_mut().unwrap() {
            let flat: Vec<f64> = serde_json::from_value(item["points"].clone()).unwrap();
            item["points"] = flat
                .chunks(2)
                .map(|pair| serde_json::json!({ "x": pair[0], "y": pair[1] }))
                .collect();
        }
        let legacy = folder.join("legacy.romo");
        atomic_write(&legacy, &serde_json::to_vec_pretty(&old).unwrap()).unwrap();
        assert_eq!(load(&legacy).unwrap().board, document.board);
        // A session from before keeps its own copy of the board.
        let session = folder.join("session.json");
        let old_session = serde_json::json!({ "document": old, "path": legacy, "dirty": false });
        atomic_write(&session, &serde_json::to_vec(&old_session).unwrap()).unwrap();
        assert_eq!(load_session(&session).unwrap().1.board, document.board);
        // Damaged files are refused.
        atomic_write(&path, &bytes[..bytes.len() / 2]).unwrap();
        assert!(load(&path).is_err());
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

use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use base64::Engine as _;
use base64::engine::general_purpose;
use chrono::DateTime;
use chrono::Utc;
use filetime::FileTime;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

// NasInfo
#[derive(Debug, Clone, PartialEq)]
pub enum NasState {
    Unsync,
    Synced,
    Syncing,
    Err,
}

#[derive(Debug, Clone)]
pub enum NasEvent {
    Onboard,
    Offboard,
}

#[derive(Debug, Clone)]
pub struct NasInfo {
    pub ts: u64,
    pub name: String,
    pub onboard: bool,
    pub nas_state: NasState,
    pub tailscale_ip: Option<String>,
}

pub fn hash_str(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    hex::encode(digest)
}

pub type FileHash = String;
pub type FileName = String;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct FileMeta {
    pub filename: FileName,
    pub hash: FileHash,
    pub mtime: SystemTime,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileList {
    pub file_list: Vec<FileMeta>,
    pub hash_str: String,
}

impl FileList {
    pub async fn new(folder: &str) -> Self {
        let mut file_list = vec![];

        for entry in WalkDir::new(folder)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let filename = format!(
                "{folder}/{}",
                entry.path().strip_prefix(folder).unwrap().to_string_lossy()
            );
            let content = fs::read(entry.path()).unwrap_or_default();
            let hash = hash_str(&String::from_utf8_lossy(&content));
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);

            file_list.push(FileMeta {
                filename,
                hash,
                mtime,
            });
        }

        file_list.sort_by(|a, b| a.filename.cmp(&b.filename));
        let serialized = file_list
            .iter()
            .map(|f| format!("{}:{}", f.filename, f.hash,))
            .collect::<Vec<_>>()
            .join("|");
        let hash_str = hash_str(&serialized);

        Self {
            file_list,
            hash_str,
        }
    }

    pub fn find_by_filename(&self, filename: &str) -> Option<&FileMeta> {
        self.file_list.iter().find(|f| f.filename == filename)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SyncAction {
    GetFile { filename: String, mtime: SystemTime },
    PutFile { filename: String, mtime: SystemTime },
}

pub fn compare_and_generate_actions(
    server_list: &FileList,
    client_list: &FileList,
) -> Vec<SyncAction> {
    let mut actions = vec![];

    for server_file in &server_list.file_list {
        match client_list.find_by_filename(&server_file.filename) {
            Some(client_file) => {
                if client_file.hash != server_file.hash || client_file.mtime != server_file.mtime {
                    let action = if client_file.mtime > server_file.mtime {
                        SyncAction::PutFile {
                            filename: server_file.filename.clone(),
                            mtime: client_file.mtime,
                        }
                    } else {
                        SyncAction::GetFile {
                            filename: server_file.filename.clone(),
                            mtime: server_file.mtime,
                        }
                    };
                    actions.push(action);
                }
            }
            None => {
                actions.push(SyncAction::GetFile {
                    filename: server_file.filename.clone(),
                    mtime: server_file.mtime,
                });
            }
        }
    }

    for client_file in &client_list.file_list {
        if server_list
            .find_by_filename(&client_file.filename)
            .is_none()
        {
            actions.push(SyncAction::PutFile {
                filename: client_file.filename.clone(),
                mtime: client_file.mtime,
            });
        }
    }

    actions
}

pub async fn write_file(filename: &str, content: &str, mtime: &str) -> anyhow::Result<()> {
    let file_path = PathBuf::from(filename);

    // if the content is the same, return
    if file_path.exists() {
        let bytes = fs::read(&file_path)?;
        let encoded = general_purpose::STANDARD.encode(&bytes);
        if encoded == content {
            return Ok(());
        }
    }

    let decoded = general_purpose::STANDARD.decode(content)?;
    let mtime: DateTime<Utc> = DateTime::parse_from_rfc3339(mtime)?.with_timezone(&Utc);

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&file_path, decoded)?;

    let file_time = FileTime::from_unix_time(mtime.timestamp(), 0);
    filetime::set_file_mtime(&file_path, file_time)?;

    Ok(())
}

pub async fn safe_remove<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();

    if !path.exists() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("Path not found: {}", path.display()),
        ));
    }

    if path.is_file() {
        fs::remove_file(path)?;
    } else if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("Not a file or directory: {}", path.display()),
        ));
    }

    Ok(())
}

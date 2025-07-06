use std::path::Path;
use std::process::Stdio;

use tokio::io;
use tokio::process::Command;
use walkdir::WalkDir;

const YT_DLP_CACHE: &str = "./yt_dlp_cache";

#[derive(Debug)]
pub struct YtDlp {
    version: String,
    output_dir: String,
}

impl YtDlp {
    pub async fn new(output_dir: String) -> Self {
        Self {
            version: String::new(),
            output_dir,
        }
    }

    fn get_command(&self) -> &'static str {
        "yt-dlp"
    }

    async fn is_available(&self) -> bool {
        let bin = self.get_command();
        Command::new(bin)
            .arg("--version")
            .output()
            .await
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    async fn get_version(&mut self) -> io::Result<String> {
        let bin = self.get_command();
        let output = Command::new(bin).arg("--version").output().await?;

        let version = String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string();

        self.version = version.clone();

        if output.status.success() {
            Ok(version)
        } else {
            Err(io::Error::other("yt-dlp execution failed"))
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub async fn init(&mut self) -> io::Result<String> {
        if !self.is_available().await {
            return Err(io::Error::new(io::ErrorKind::NotFound, "yt-dlp not found"));
        }

        let version = self.get_version().await?;

        Ok(version)
    }

    pub async fn download(&mut self, url: &str) -> Result<(), String> {
        let _ = remove_dir(YT_DLP_CACHE);
        let _ = std::fs::create_dir_all(YT_DLP_CACHE);

        let output_file = format!("{YT_DLP_CACHE}/%(title)s.%(ext)s");

        let status = Command::new("yt-dlp")
            .args([
                "--output",
                &output_file,
                "--embed-thumbnail",
                "--add-metadata",
                "--extract-audio",
                "--audio-format",
                "mp3",
                "--audio-quality",
                "320K",
                url,
            ])
            .stdout(Stdio::null()) // 隱藏標準輸出
            .stderr(Stdio::null()) // 隱藏錯誤輸出
            .status()
            .await;

        match status {
            Ok(status) if status.success() => {
                let _ = move_music(YT_DLP_CACHE, &self.output_dir);
                let _ = remove_dir(YT_DLP_CACHE);
                Ok(())
            }
            Ok(_) => {
                let _ = remove_dir(YT_DLP_CACHE);
                Err(format!("Failed to download {output_file}"))
            }
            Err(_) => {
                let _ = remove_dir(YT_DLP_CACHE);
                Err("Failed to execute yt-dlp".to_string())
            }
        }
    }
}

fn move_music(source_dir: &str, target_dir: &str) -> std::io::Result<()> {
    let source_dir = Path::new(source_dir);
    let target_dir = Path::new(target_dir);

    // 確保目標資料夾存在
    if !target_dir.exists() {
        std::fs::create_dir_all(target_dir)?;
    }

    // 遍歷 source_dir 底下的所有檔案
    for entry in WalkDir::new(source_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let relative_path = entry.path().strip_prefix(source_dir).unwrap();
        let target_path = target_dir.join(relative_path);

        // 建立目標子資料夾（如果有）
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 搬移檔案
        std::fs::rename(entry.path(), &target_path)?;
    }

    Ok(())
}

fn remove_dir(remove_dir: &str) -> std::io::Result<()> {
    let dir_to_remove = Path::new(remove_dir);

    if dir_to_remove.exists() {
        std::fs::remove_dir_all(dir_to_remove)?;
    }

    Ok(())
}

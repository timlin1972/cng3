use std::{env, io};

use tokio::process::Command;

#[derive(Debug)]
pub struct YtDlp {}

impl YtDlp {
    pub async fn new() -> Self {
        Self {}
    }

    fn get_command(&self) -> &'static str {
        if cfg!(windows) {
            "yt-dlp.exe"
        } else {
            "yt-dlp"
        }
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

    async fn get_version(&self) -> io::Result<String> {
        let bin = self.get_command();
        let output = Command::new(bin).arg("--version").output().await?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "yt-dlp execution failed",
            ))
        }
    }

    pub async fn init(&mut self) -> io::Result<String> {
        if !self.is_available().await {
            println!("yt-dlp not found.");
            return Err(io::Error::new(io::ErrorKind::NotFound, "yt-dlp not found"));
        }

        let version = self.get_version().await?;
        println!("yt-dlp OK, version: {}", version.trim());

        Ok(version)
    }
}

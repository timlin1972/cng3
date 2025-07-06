use std::io;

use tokio::process::Command;

#[derive(Debug)]
pub struct Ffmpeg {
    version: String,
}

impl Ffmpeg {
    pub async fn new() -> Self {
        Self {
            version: String::new(),
        }
    }

    fn get_command(&self) -> &'static str {
        "ffmpeg"
    }

    async fn is_available(&self) -> bool {
        let bin = self.get_command();
        Command::new(bin)
            .arg("-version")
            .output()
            .await
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    pub async fn get_version(&mut self) -> io::Result<String> {
        let bin = self.get_command();
        let output = Command::new(bin).arg("-version").output().await?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first_line) = stdout.lines().next() {
                // 通常第一行像這樣：ffmpeg version 4.4.2-0ubuntu0.22.04.1 ...
                if let Some(version) = first_line.split_whitespace().nth(2) {
                    self.version = version.to_string();
                    return Ok(version.to_string());
                }
            }
            Err(io::Error::other("Unable to extract ffmpeg version"))
        } else {
            Err(io::Error::other("ffmpeg execution failed"))
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub async fn init(&mut self) -> io::Result<String> {
        if !self.is_available().await {
            return Err(io::Error::new(io::ErrorKind::NotFound, "ffmpeg not found"));
        }

        let version = self.get_version().await?;

        Ok(version)
    }
}

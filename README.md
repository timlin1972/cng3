# cng3

## CLI mode

```
cargo run -- --script cli.scripts
```

## GUI mode

```
cargo run -- --script gui.scripts
```

# yt-dlp

## GNU/Linux

```
sudo wget https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -O /usr/local/bin/yt-dlp
sudo chmod a+rx /usr/local/bin/yt-dlp
```

yt-dlp 日後可以自己更新自己：

```
sudo yt-dlp -U
```

ffmpeg 可以從套件管理器裝：

```
sudo apt install ffmpeg
```

確認軟體版本：

```
yt-dlp --version
ffmpeg -version
```

## MacOS

開啟終端機，安裝 [Homebrew](https://brew.sh/)

輸入以下指令安裝 yt-dlp 和 ffmpeg：

```
brew install yt-dlp ffmpeg
```

日後更新指令：

```
brew upgrade yt-dlp
```

確認軟體版本：

```
yt-dlp --version
ffmpeg -version
```

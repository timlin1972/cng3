use std::{
    fs,
    path::{Path, PathBuf},
};

use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use base64::Engine as _;
use base64::engine::general_purpose;
use chrono::{DateTime, Utc};
use log::Level::{self, Info, Warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender; // trait for `.encode()`

use crate::consts::{self, WEB_PORT};
use crate::messages::{ACTION_NAS_STATE, Cmd, Data, Log, Msg};
use crate::utils::{self, FileList};

const MODULE: &str = "web";
const MAX_SIZE: usize = 50 * 1024 * 1024; // 50MB

#[get("/")]
async fn hello() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
}

#[derive(Deserialize)]
struct CheckHashRequest {
    data: CheckHashData,
}
#[derive(Deserialize)]
struct CheckHashData {
    name: String,
    hash_str: String,
}

#[post("/check_hash")]
pub async fn check_hash(
    data: web::Json<CheckHashRequest>,
    msg_tx: web::Data<Sender<Msg>>,
) -> impl Responder {
    let name = &data.data.name;
    let hash_str = &data.data.hash_str;

    // get local file_list
    let file_list = FileList::new(consts::NAS_FOLDER).await;

    let hash_str_same = hash_str == &file_list.hash_str;

    let hash_str_result = if hash_str_same { "Same" } else { "Different" };

    info(
        &msg_tx,
        format!("[{MODULE}] API: check_hash: {name}, result: {hash_str_result}",),
    )
    .await;

    let msg = Msg {
        ts: utils::ts(),
        module: MODULE.to_string(),
        data: Data::Cmd(Cmd {
            cmd: format!(
                "p nas {ACTION_NAS_STATE} {name} {}",
                if hash_str_same { "Synced" } else { "Syncing" }
            ),
        }),
    };
    let _ = msg_tx.send(msg).await;

    if hash_str_same {
        HttpResponse::Ok().json(json!({
            "data": {
                "result": 0
            }
        }))
    } else {
        HttpResponse::Ok().json(json!({
            "data": {
                "result": 1,
                "file_list": file_list
            }
        }))
    }
}

#[derive(Deserialize)]
struct UploadRequest {
    data: UploadData,
}

#[derive(Deserialize)]
struct UploadData {
    filename: String,
    content: String,
    mtime: String,
}

#[post("/upload")]
async fn upload(data: web::Json<UploadRequest>, msg_tx: web::Data<Sender<Msg>>) -> impl Responder {
    let filename = &data.data.filename;
    if !is_valid_filename(filename) {
        return HttpResponse::BadRequest().body("Invalid filename");
    }

    let content = &data.data.content;
    let mtime = &data.data.mtime;

    if let Err(e) = utils::write_file(filename, content, mtime).await {
        warn(
            &msg_tx,
            format!("[{MODULE}] Failed to write `{filename}`: {e}"),
        )
        .await;
        return HttpResponse::InternalServerError().body("Failed to write `{filename}`: {e}");
    }

    info(&msg_tx, format!("[{MODULE}] API: upload `{filename}`")).await;

    HttpResponse::Ok().finish()
}

#[derive(Deserialize)]
struct RemoveRequest {
    data: RemoveData,
}

#[derive(Deserialize)]
struct RemoveData {
    filename: String,
}

#[post("/remove")]
async fn remove(data: web::Json<RemoveRequest>, msg_tx: web::Data<Sender<Msg>>) -> impl Responder {
    let filename = &data.data.filename;
    if !is_valid_filename(filename) {
        return HttpResponse::BadRequest().body("Invalid filename");
    }

    if let Err(e) = utils::safe_remove(filename).await {
        warn(
            &msg_tx,
            format!("[{MODULE}] Failed to remove `{filename}`: {e}"),
        )
        .await;
        return HttpResponse::InternalServerError().body("Failed to remove `{filename}`: {e}");
    }

    info(&msg_tx, format!("[{MODULE}] API: REMOVE `{filename}`")).await;

    HttpResponse::Ok().finish()
}

#[derive(Deserialize)]
struct DownloadRequest {
    data: DownloadData,
}

#[derive(Deserialize)]
struct DownloadData {
    filename: String,
}

#[derive(Serialize)]
struct DownloadResponse {
    data: DownloadResponseData,
}
#[derive(Serialize)]
struct DownloadResponseData {
    filename: String,
    content: String,
    mtime: String,
}

#[post("/download")]
async fn download(
    data: web::Json<DownloadRequest>,
    msg_tx: web::Data<Sender<Msg>>,
) -> impl Responder {
    let filename = &data.data.filename;
    if !is_valid_filename(filename) {
        return HttpResponse::BadRequest().body("Invalid filename");
    }

    let path = PathBuf::from(filename);

    match fs::read(&path) {
        Ok(bytes) => {
            let mtime = fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .map(|time| DateTime::<Utc>::from(time).to_rfc3339())
                .unwrap_or_else(|_| Utc::now().to_rfc3339());

            let encoded = general_purpose::STANDARD.encode(&bytes);

            info(&msg_tx, format!("[{MODULE}] API: GET `{filename}`")).await;

            HttpResponse::Ok().json(DownloadResponse {
                data: DownloadResponseData {
                    filename: filename.clone(),
                    content: encoded,
                    mtime,
                },
            })
        }
        Err(_) => HttpResponse::NotFound().json(json!({
            "error": "Not Found",
            "message": "指定的資源不存在"
        })),
    }
}

pub struct Web {
    msg_tx: Sender<Msg>,
    shutdown_tx: broadcast::Sender<()>,
}

impl Web {
    pub async fn new(msg_tx: Sender<Msg>, shutdown_tx: broadcast::Sender<()>) -> Self {
        Self {
            msg_tx,
            shutdown_tx,
        }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let msg_tx_clone = self.msg_tx.clone();

        let server = HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(msg_tx_clone.clone()))
                .app_data(web::PayloadConfig::new(MAX_SIZE))
                .app_data(web::JsonConfig::default().limit(MAX_SIZE))
                .service(hello)
                .service(download)
                .service(upload)
                .service(remove)
                .service(check_hash)
        })
        .bind(("0.0.0.0", WEB_PORT))?
        .run();

        let handle = server.handle();

        let server_task = tokio::spawn(server);

        let shutdown_task = tokio::spawn(async move {
            if shutdown_rx.recv().await.is_ok() {
                handle.stop(true).await;
            }
        });

        let _ = tokio::try_join!(server_task, shutdown_task);

        Ok(())
    }
}

async fn log(msg_tx: &Sender<Msg>, level: Level, msg: String) {
    let msg = Msg {
        ts: utils::ts(),
        module: MODULE.to_string(),
        data: Data::Log(Log { level, msg }),
    };
    let _ = msg_tx.send(msg).await;
}

async fn info(msg_tx: &Sender<Msg>, msg: String) {
    log(msg_tx, Info, msg).await;
}

async fn warn(msg_tx: &Sender<Msg>, msg: String) {
    log(msg_tx, Warn, msg).await;
}

fn is_valid_filename(path: &str) -> bool {
    let path = Path::new(path);
    path.components().all(|c| {
        matches!(
            c,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    }) && !path.is_absolute()
}

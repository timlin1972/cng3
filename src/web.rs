use std::rc::Rc;
use std::task::{Context, Poll};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use actix_files::Files;
use actix_multipart::Multipart;
use actix_web::{
    App, Error, HttpResponse, HttpServer, Responder,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    get,
    http::header::CONTENT_TYPE,
    post, web,
};
use base64::Engine as _;
use base64::engine::general_purpose;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use futures_util::future::{LocalBoxFuture, Ready, ok};
use log::Level::{self, Info, Warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender; // trait for `.encode()`

use crate::consts::{self, NAS_FOLDER, NAS_NAME, UPLOAD_FOLDER, WEB_PORT};
use crate::messages::{ACTION_NAS_STATE, Cmd, Data, Log, Msg};
use crate::utils::{
    self,
    nas_info::{self, FileList},
};

const MODULE: &str = "web";
const MAX_SIZE: usize = 100 * 1024 * 1024; // 100MB
const API_V1_UPLOAD: &str = "/api/v1/upload";

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
        ts: utils::time::ts(),
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
struct VerifyHashRequest {
    data: VerifyHashData,
}
#[derive(Deserialize)]
struct VerifyHashData {
    filename: String,
    hash_str: String,
}

#[post("/verify_hash")]
pub async fn verify_hash(
    data: web::Json<VerifyHashRequest>,
    msg_tx: web::Data<Sender<Msg>>,
) -> impl Responder {
    let mut result = 1;

    let filename = &data.data.filename;
    let hash_str = &data.data.hash_str;

    let file_path = PathBuf::from(filename);

    if let Ok(bytes) = fs::read(&file_path) {
        let hash_str_local = nas_info::hash_str(&String::from_utf8_lossy(&bytes));

        if *hash_str == hash_str_local {
            result = 0;
        }

        let hash_str_result = if *hash_str == hash_str_local {
            "Same"
        } else {
            "Different"
        };

        info(
            &msg_tx,
            format!("[{MODULE}] API: verify_hash: `{filename}`, result: {hash_str_result}",),
        )
        .await;
    }

    HttpResponse::Ok().json(json!({
        "data": {
            "result": result
        }
    }))
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

    if let Err(e) = nas_info::write_file(filename, content, mtime).await {
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

    if let Err(e) = nas_info::safe_remove(filename).await {
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

async fn upload_file(mut payload: Multipart, msg_tx: web::Data<Sender<Msg>>) -> impl Responder {
    while let Some(Ok(mut field)) = payload.next().await {
        let filename = field
            .content_disposition()
            .and_then(|cd| cd.get_filename())
            .map(sanitize_filename::sanitize)
            .unwrap_or_else(|| format!("upload-{}.bin", uuid::Uuid::new_v4()));

        let _ = fs::create_dir_all(UPLOAD_FOLDER);

        let filepath = format!("{UPLOAD_FOLDER}/{filename}");
        info(&msg_tx, format!("[{MODULE}] API: upload_file: {filepath}")).await;

        let start_ts = utils::time::ts();

        let mut f = match File::create(&filepath) {
            Ok(file) => file,
            Err(e) => {
                return HttpResponse::InternalServerError()
                    .body(format!("Failed to create file. Err: {e}"));
            }
        };

        while let Some(Ok(chunk)) = field.next().await {
            if let Err(e) = f.write_all(&chunk) {
                return HttpResponse::InternalServerError()
                    .body(format!("Failed to write file. Err: {e}"));
            }
        }

        let escaped_time = utils::time::ts() - start_ts;
        info(
            &msg_tx,
            format!(
                "[{MODULE}] API: upload_file: {filepath}, escaped: {}",
                utils::time::transmit_str(f.metadata().unwrap().len(), escaped_time)
            ),
        )
        .await;
    }

    HttpResponse::Ok().body("Upload complete")
}

#[derive(Clone)]
struct CharsetMiddleware;

impl<S, B> Transform<S, ServiceRequest> for CharsetMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = CharsetMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(CharsetMiddlewareService {
            service: Rc::new(service),
        })
    }
}

struct CharsetMiddlewareService<S> {
    service: Rc<S>,
}
impl<S, B> Service<ServiceRequest> for CharsetMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);

        Box::pin(async move {
            let mut res = service.call(req).await?;

            if let Some(content_type) = res.headers().get(CONTENT_TYPE) {
                #[allow(clippy::collapsible_if)]
                if let Ok(content_type_str) = content_type.to_str() {
                    if content_type_str.starts_with("text/")
                        && !content_type_str.contains("charset")
                    {
                        let new_header = format!("{}; charset=utf-8", content_type_str);
                        res.headers_mut()
                            .insert(CONTENT_TYPE, new_header.parse().unwrap());
                    }
                }
            }

            Ok(res)
        })
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
                .route(API_V1_UPLOAD, web::post().to(upload_file))
                .service(hello)
                .service(download)
                .service(upload)
                .service(remove)
                .service(check_hash)
                .service(verify_hash)
                .wrap(CharsetMiddleware)
                .service(
                    Files::new(NAS_NAME, NAS_FOLDER)
                        .show_files_listing()
                        .prefer_utf8(true),
                )
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
        ts: utils::time::ts(),
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

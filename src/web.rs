use actix_web::{App, HttpResponse, HttpServer, Responder, get};
use tokio::sync::broadcast;

#[get("/")]
async fn hello() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
}

pub struct Web {
    shutdown_tx: broadcast::Sender<()>,
}

impl Web {
    pub async fn new(shutdown_tx: broadcast::Sender<()>) -> Self {
        Self { shutdown_tx }
    }

    pub async fn run(&mut self) -> std::io::Result<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let server = HttpServer::new(|| App::new().service(hello))
            .bind(("0.0.0.0", 8080))?
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

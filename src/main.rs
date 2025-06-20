use tokio::sync::broadcast;

mod app;
mod messages;
mod plugins;
mod utils;
mod web;

fn handle_panic() {
    std::panic::set_hook(Box::new(|info| {
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            *s
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.as_str()
        } else {
            "Unknown panic message"
        };

        let location = info
            .location()
            .map(|l| format!("at {}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".into());

        eprintln!("💥 Panic occurred: '{}' {}", message, location);

        std::process::exit(1);
    }));
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    handle_panic();

    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    app::App::new(shutdown_tx.clone()).await.run().await?;
    web::Web::new(shutdown_tx.clone()).await.run().await?;

    Ok(())
}

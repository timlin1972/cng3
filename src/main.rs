use std::env;

use anyhow::Result;
use tokio::sync::{broadcast, mpsc};

mod app;
mod cfg;
mod consts;
mod messages;
mod plugins;
mod utils;
mod web;

use messages::Msg;

const SCRIPTS_FILENAME: &str = "./init.scripts";
const MSG_SIZE: usize = 4096;
const SCRIPT_FLAG: &str = "--script";

fn handle_panic() {
    std::panic::set_hook(Box::new(|info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("Unknown panic message");

        let location = info
            .location()
            .map(|l| format!("at {}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());

        eprintln!("💥 Panic occurred: '{message}' {location}");

        std::process::exit(1);
    }));
}

fn parse_args(args: &mut impl Iterator<Item = String>) -> Result<String, &'static str> {
    let mut scripts_filename = SCRIPTS_FILENAME.to_string();

    while let Some(arg) = args.next() {
        if arg == SCRIPT_FLAG {
            if let Some(path) = args.next() {
                scripts_filename = path;
            } else {
                return Err("Missing value after `--script`");
            }
        }
    }

    Ok(scripts_filename)
}

#[actix_web::main]
async fn main() -> Result<()> {
    handle_panic();

    let mut args = env::args().skip(1);
    let scripts_filename = parse_args(&mut args).unwrap_or_else(|e| {
        eprintln!("❌ Error: {e}");
        std::process::exit(1);
    });

    let (msg_tx, msg_rx) = mpsc::channel::<Msg>(MSG_SIZE);
    let (shutdown_notify, _) = broadcast::channel::<()>(1);

    app::App::new(
        msg_tx.clone(),
        msg_rx,
        shutdown_notify.clone(),
        scripts_filename,
    )
    .await
    .run()
    .await?;

    web::Web::new(msg_tx.clone(), shutdown_notify.clone())
        .await
        .run()
        .await?;

    Ok(())
}

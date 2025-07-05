use tokio::sync::mpsc::Sender;

use crate::messages::{Cmd, Data, Log, Msg};
use crate::utils::{self, mode::Mode};

// Panel
pub async fn output_update_gui_simple(
    module: &str,
    msg_tx: &Sender<Msg>,
    gui_panel: &str,
    output: String,
) {
    let ts = utils::time::ts();
    let module = module.to_string();

    let msg = Msg {
        ts,
        module,
        data: Data::Cmd(Cmd {
            cmd: format!("p panels output_update {gui_panel} '{output}'"),
        }),
    };
    let _ = msg_tx.send(msg).await;
}

pub async fn output_push(
    module: &str,
    msg_tx: &Sender<Msg>,
    mode: &Mode,
    gui_panel: &str,
    level: log::Level,
    output: String,
) {
    let ts = utils::time::ts();
    let module = module.to_string();
    match mode {
        Mode::ModeGui => {
            let msg = Msg {
                ts,
                module,
                data: Data::Cmd(Cmd {
                    cmd: format!(
                        "p panels output_push {gui_panel} '{} [{level}] {output}'",
                        utils::time::ts_str(ts)
                    ),
                }),
            };
            let _ = msg_tx.send(msg).await;
        }
        Mode::ModeCli => {
            let msg = Msg {
                ts,
                module,
                data: Data::Log(Log { level, msg: output }),
            };
            let _ = msg_tx.send(msg).await;
        }
    }
}

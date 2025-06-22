use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

const DEF_NAME: &str = "cng3_default";
const CFG_FILE: &str = "./cfg.json";

static INSTANCE: Lazy<Mutex<Cfg>> = Lazy::new(|| Mutex::new(Cfg::new()));

fn default_name() -> String {
    DEF_NAME.to_string()
}

#[derive(Serialize, Deserialize)]
pub struct Cfg {
    #[serde(default = "default_name")]
    name: String,
}

impl Cfg {
    pub fn new() -> Self {
        let path = Path::new(CFG_FILE);

        let cfg = if !path.exists() {
            Cfg {
                name: DEF_NAME.to_owned(),
            }
        } else {
            let file_content = fs::read_to_string(CFG_FILE).unwrap();
            serde_json::from_str(&file_content).unwrap()
        };

        let file_content = serde_json::to_string_pretty(&cfg).unwrap();
        let mut file = File::create(CFG_FILE).unwrap();
        file.write_all(file_content.as_bytes()).unwrap();

        cfg
    }

    fn get_instance() -> std::sync::MutexGuard<'static, Cfg> {
        INSTANCE.lock().unwrap()
    }

    fn name(&self) -> &str {
        &self.name
    }
}

pub fn name() -> String {
    let cfg = Cfg::get_instance();
    cfg.name().to_owned()
}

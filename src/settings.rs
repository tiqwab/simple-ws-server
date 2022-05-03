use anyhow::Result;
use config::Config;
use getset::Getters;
use human_size::Size;
use serde::Deserialize;
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Deserialize, Getters)]
pub struct Http {
    #[serde(default)]
    #[getset(get = "pub")]
    addr: String,
    #[serde(default)]
    #[getset(get = "pub")]
    port: u16,
}

impl Default for Http {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1".to_string(),
            port: 8888,
        }
    }
}

#[derive(Debug, Deserialize, Getters)]
pub struct WebSocket {
    #[serde(default = "default_max_payload_size")]
    #[getset(get = "pub")]
    max_payload_size: Size,
}

fn default_max_payload_size() -> Size {
    Size::from_str("1MB").unwrap()
}

impl Default for WebSocket {
    fn default() -> Self {
        Self {
            max_payload_size: default_max_payload_size(),
        }
    }
}

#[derive(Debug, Deserialize, Getters, Default)]
pub struct Settings {
    #[getset(get = "pub")]
    http: Http,
    #[getset(get = "pub")]
    ws: WebSocket,
}

impl Settings {
    pub fn load(path: impl AsRef<Path>) -> Result<Settings> {
        let config = Config::builder()
            .add_source(config::File::from(path.as_ref()))
            .add_source(config::Environment::with_prefix("SWS").separator("__"))
            .build()?;
        let settings = config.try_deserialize()?;
        Ok(settings)
    }
}

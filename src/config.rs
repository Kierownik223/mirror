use std::fs;

use serde::{Deserialize, Serialize};


#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Config {
    pub extensions: Vec<String>,
    pub hidden_files: Vec<String>,
    pub enable_login: bool,
    pub enable_api: bool,
    pub enable_marmak_link: bool,
    pub enable_direct: bool,
    pub instance_info: String,
    pub x_sendfile_header: String,
    pub x_sendfile_prefix: String,
    pub standalone: bool,
    pub fallback_root_domain: String,
    pub enable_file_db: bool,
    pub enable_zip_downloads: bool,
    pub max_age: u64,
}

impl Config {
    pub fn load() -> Self {
        let config_str = fs::read_to_string("config.toml").unwrap_or_default();
        toml::from_str(&config_str).unwrap_or_default()
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            extensions: vec![
                "exe".into(), "cab".into(), "appx".into(), "xap".into(), "appxbundle".into(), "zip".into(), "7z".into(), "apk".into(), "rar".into(),
            ],
            hidden_files: vec![
                "static".into(), "uploads".into(), "private".into(), "robots.txt".into(), "favicon.ico".into(), "top".into(), "RESTRICTED".into(), "metadata".into(), "HIDDEN".into(),
            ],
            enable_login: false,
            enable_api: true,
            enable_marmak_link: true,
            enable_direct: false,
            instance_info: "My Mirror Instance!".into(),
            x_sendfile_header: "X-Send-File".into(),
            x_sendfile_prefix: String::new(),
            standalone: true,
            fallback_root_domain: "marmak.net.pl".into(),
            enable_file_db: false,
            enable_zip_downloads: false,
            max_age: 86400,
        }
    }
}
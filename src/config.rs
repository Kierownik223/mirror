use std::{collections::HashMap, env, fs};

use once_cell::sync::Lazy;
use rocket::data::ToByteUnit;
use serde::{Deserialize, Serialize};

use crate::utils::parse_bool;

pub static CONFIG: Lazy<Config> = Lazy::new(Config::load);

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Config {
    pub extensions: Vec<String>,
    pub hidden_files: Vec<String>,
    pub enable_login: bool,
    pub enable_api: bool,
    pub marmak_link: Option<String>,
    pub instance_info: String,
    pub x_sendfile_header: String,
    pub x_sendfile_prefix: String,
    pub standalone: bool,
    pub fallback_root_domain: String,
    pub enable_file_db: bool,
    pub enable_zip_downloads: bool,
    pub max_age: u64,
    pub static_max_age: u64,
    pub jwt_secret: String,
    pub linkshortener: bool,
    pub linkshortener_url: String,
    pub max_upload_sizes: HashMap<String, u64>,
    pub private_folder_quotas: HashMap<String, u64>,
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
            extensions: serde_json::from_str(&env::var("MIRROR_EXTENSIONS").unwrap_or("[\"exe\",\"cab\",\"appx\",\"xap\",\"appxbundle\",\"zip\",\"7z\",\"apk\",\"rar\"]".into())).unwrap_or(vec![
                "exe".into(),
                "cab".into(),
                "appx".into(),
                "xap".into(),
                "appxbundle".into(),
                "zip".into(),
                "7z".into(),
                "apk".into(),
                "rar".into(),
            ]),
            hidden_files: serde_json::from_str(&env::var("MIRROR_HIDDEN_FILES").unwrap_or("[\"static\",\"uploads\",\"private\",\"robots.txt\",\"favicon.ico\",\"top\",\"RESTRICTED\",\"metadata\",\"HIDDEN\"]".into())).unwrap_or(vec![
                "static".into(),
                "uploads".into(),
                "private".into(),
                "robots.txt".into(),
                "favicon.ico".into(),
                "top".into(),
                "RESTRICTED".into(),
                "metadata".into(),
                "HIDDEN".into(),
            ]),
            enable_login: parse_bool(&env::var("MIRROR_ENABLE_LOGIN").unwrap_or("false".into())),
            enable_api: parse_bool(&env::var("MIRROR_ENABLE_API").unwrap_or("true".into())),
            marmak_link:  env::var("MIRROR_MARMAK_LINK").ok(),
            instance_info: env::var("MIRROR_INSTANCE_INFO").unwrap_or("My Mirror Instance!".into()),
            x_sendfile_header: env::var("MIRROR_X_SENDFILE_HEADER").unwrap_or("X-Send-File".into()),
            x_sendfile_prefix: env::var("MIRROR_X_SENDFILE_PREFIX").unwrap_or("".into()),
            standalone: parse_bool(&env::var("MIRROR_STANDALONE").unwrap_or("false".into())),
            fallback_root_domain: env::var("MIRROR_FALLBACK_ROOT_DOMAIN").unwrap_or("marmak.net.pl".into()),
            enable_file_db: parse_bool(&env::var("MIRROR_ENABLE_FILE_DB").unwrap_or("false".into())),
            enable_zip_downloads: parse_bool(&env::var("MIRROR_ENABLE_ZIP_DOWNLOADS").unwrap_or("false".into())),
            max_age: env::var("MIRROR_MAX_AGE").unwrap_or("86400".into()).parse::<u64>().unwrap_or(86400),
            static_max_age: env::var("MIRROR_STATIC_MAX_AGE").unwrap_or("604800".into()).parse::<u64>().unwrap_or(604800),
            jwt_secret: env::var("MIRROR_JWT_SECRET").unwrap_or("".into()),
            linkshortener: parse_bool(&env::var("MIRROR_LINKSHORTENER").unwrap_or("true".into())),
            linkshortener_url: env::var("MIRROR_JWT_SECRET").unwrap_or("https://short.marmak.net.pl/api/url".into()),
            max_upload_sizes: HashMap::from([
                ("0".into(), 5.gigabytes().as_u64()),
                ("1".into(), 500.megabytes().as_u64()),
                ("2".into(), 5.gigabytes().as_u64()),
            ]),
            private_folder_quotas: HashMap::from([
                ("0".into(), 0_u64),
                ("1".into(), 2.gigabytes().as_u64()),
                ("2".into(), 50.gigabytes().as_u64()),
            ]),
        }
    }
}

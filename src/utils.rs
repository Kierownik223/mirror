use std::{
    ffi::OsStr,
    fs,
    io::{Cursor, Error},
    path::{Path, PathBuf},
    sync::Arc,
};

use humansize::{format_size, DECIMAL};
use rocket::{
    fs::NamedFile,
    http::{Cookie, CookieJar, Status},
};
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;
use zip::write::SimpleFileOptions;

use crate::{Config, FileEntry, HeaderFile, IndexResponse, MirrorFile};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct FolderSize {
    size: u64,
    file: String,
}

pub fn read_dirs(path: &str) -> Result<Vec<MirrorFile>, Error> {
    let mut dir_list = Vec::new();

    let paths = match fs::read_dir(format!("files{}", &path)) {
        Ok(paths) => paths,
        Err(e) => return Err(e),
    };

    for file_path in paths {
        let file_path = file_path?;
        let md = fs::metadata(file_path.path())?;

        if md.is_dir() {
            if let Some(file_name) = file_path.file_name().to_str() {
                let file: MirrorFile = MirrorFile {
                    name: file_name.to_owned(),
                    ext: "folder".to_string(),
                    icon: "folder".to_string(),
                    size: "---".to_string(),
                    downloads: None,
                };

                dir_list.push(file);
            }
        }
    }

    Ok(dir_list)
}

pub async fn read_dirs_async(
    path: &str,
    sizes_state: &Arc<RwLock<Vec<FileEntry>>>,
) -> Result<Vec<MirrorFile>, Error> {
    let mut dir_list = Vec::new();

    let size_list = sizes_state.read().await;

    let base_path = format!("files{}", path);
    let paths = match fs::read_dir(&base_path) {
        Ok(paths) => paths,
        Err(e) => return Err(e),
    };

    'main: for file_path in paths {
        let file_path = file_path?;
        let metadata = fs::metadata(file_path.path())?;

        if metadata.is_dir() {
            let full_path = file_path.path();

            let subdir_paths = match fs::read_dir(&full_path) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for subdir_path in subdir_paths {
                let subdir_path = subdir_path?;
                if let Some(file_name) = subdir_path.file_name().to_str() {
                    if file_name == "HIDDEN" {
                        continue 'main;
                    }
                }
            }

            let rel_path = full_path
                .strip_prefix(std::env::current_dir()?)
                .unwrap_or(&full_path)
                .display()
                .to_string();

            let folder_size = size_list
                .iter()
                .find(|entry| entry.file == rel_path)
                .map(|entry| entry.size)
                .unwrap_or(0);

            if let Some(file_name) = file_path.file_name().to_str() {
                dir_list.push(MirrorFile {
                    name: file_name.to_string(),
                    ext: "folder".to_string(),
                    icon: "folder".to_string(),
                    size: format_size(folder_size, DECIMAL),
                    downloads: None,
                });
            }
        }
    }

    Ok(dir_list)
}

pub fn read_files(path: &str) -> Result<Vec<MirrorFile>, Error> {
    let mut file_list = Vec::new();

    let paths = match fs::read_dir(format!("files{}", &path)) {
        Ok(paths) => paths,
        Err(e) => return Err(e),
    };

    for file_path in paths {
        let file_path = file_path?;
        let md = fs::metadata(file_path.path())?;

        if md.is_file() {
            if let Some(file_name) = file_path.file_name().to_str() {
                let mut icon = get_extension_from_filename(file_name)
                    .unwrap_or_else(|| "")
                    .to_string()
                    .to_lowercase();
                if !Path::new(
                    &("files/static/images/icons/".to_owned() + &icon + ".png").to_string(),
                )
                .exists()
                {
                    icon = "default".to_string();
                }
                let file: MirrorFile = MirrorFile {
                    name: file_name.to_owned(),
                    ext: get_extension_from_filename(file_name)
                        .unwrap_or_else(|| "")
                        .to_string()
                        .to_lowercase(),
                    icon: icon,
                    size: format_size(md.len(), DECIMAL),
                    downloads: None,
                };

                file_list.push(file);
            }
        }
    }

    Ok(file_list)
}

pub fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename).extension().and_then(OsStr::to_str)
}

pub fn get_bool_cookie(jar: &CookieJar<'_>, name: &str, default: bool) -> bool {
    jar.get(name)
        .map(|c| c.value() == "true")
        .unwrap_or(default)
}

pub fn get_session<'a>(jar: &CookieJar<'_>) -> (String, i32) {
    if is_logged_in(jar) {
        let session = jar
            .get_private("session")
            .map(|cookie| cookie.value().to_string())
            .unwrap_or_else(|| "defaultuser.1".to_string());

        let user_name: Vec<&str> = session.split(".").collect();
        let username = user_name[0].to_string();
        let perms = str::parse::<i32>(user_name[1]).unwrap();

        (username, perms)
    } else {
        ("Nobody".to_string(), 1)
    }
}

pub fn get_theme<'a>(jar: &CookieJar<'_>) -> String {
    let mut theme = jar
        .get("theme")
        .map(|cookie| cookie.value())
        .unwrap_or("standard");

    if !Path::new(&format!("files/static/styles/{}.css", &theme)).exists() {
        theme = "standard";
    }

    theme.to_string()
}

pub fn is_logged_in(jar: &CookieJar<'_>) -> bool {
    jar.get_private("session").is_some()
}

pub fn is_restricted(path: &Path, jar: &CookieJar<'_>) -> bool {
    let mut current = Some(path);

    while let Some(p) = current {
        if p.join("RESTRICTED").exists() {
            return !is_logged_in(jar);
        }
        current = p.parent();
    }

    false
}

pub fn is_hidden(path: &Path, jar: &CookieJar<'_>) -> bool {
    let mut current = Some(path);

    while let Some(p) = current {
        if p.join("HIDDEN").exists() {
            if is_logged_in(jar) {
                let (_, perms) = get_session(jar);
                return perms != 0;
            } else {
                return true;
            }
        }
        current = p.parent();
    }

    false
}

pub async fn open_file(path: PathBuf, cache: bool) -> Result<IndexResponse, Status> {
    let config = Config::load();
    if !path.exists() {
        return Err(Status::NotFound);
    }

    if config.standalone {
        match NamedFile::open(&path).await {
            Ok(f) => Ok(IndexResponse::NamedFile(f)),
            Err(_) => Err(Status::InternalServerError),
        }
    } else {
        Ok(IndexResponse::HeaderFile(HeaderFile(
            path.display().to_string(),
            cache,
        )))
    }
}

pub fn list_to_files(files: Vec<&str>) -> Result<Vec<MirrorFile>, Error> {
    let mut file_list = Vec::new();

    for file in files {
        let mut ext = get_extension_from_filename(file)
            .unwrap_or_else(|| "")
            .to_lowercase();

        if file.ends_with("/") {
            ext = "Folder".to_string();
        }

        let mut icon = ext.as_str();
        if !Path::new(
            &format!("files/static/images/icons/{}.png", &icon),
        )
        .exists()
        {
            icon = "default";
        }

        let file: MirrorFile = MirrorFile {
            name: file.to_string(),
            ext: ext.to_string(),
            icon: icon.to_string(),
            size: "---".to_string(),
            downloads: None,
        };

        file_list.push(file);
    }

    Ok(file_list)
}

pub fn create_cookie<'a>(name: &'a str, value: &str) -> Cookie<'a> {
    let now = OffsetDateTime::now_utc() + Duration::days(365);
    let mut cookie = Cookie::new(name, value.to_string());
    cookie.set_expires(now);
    cookie
}

pub fn parse_language(header: &str) -> Option<String> {
    let lang_dir = "lang/";

    for lang in header.split(',') {
        let code = lang.split(';').next()?.trim();
        let short_code = code.split('-').next()?.to_lowercase();

        if fs::metadata(format!("{}/{}.toml", lang_dir, short_code)).is_ok() {
            return Some(short_code);
        }
    }

    None
}

pub fn get_root_domain<'a>(host: &str, fallback: &str) -> String {
    return host.splitn(2, '.').nth(1).unwrap_or(fallback).to_string();
}

pub fn add_path_to_zip(
    zip_writer: &mut zip::ZipWriter<&mut Cursor<Vec<u8>>>,
    base_path: &Path,
    path: &Path,
    options: SimpleFileOptions,
) -> std::io::Result<()> {
    if path.is_file() {
        let mut file = std::fs::File::open(path)?;
        let relative_path = path.strip_prefix(base_path).unwrap_or(path);
        zip_writer.start_file(relative_path.to_string_lossy(), options)?;
        std::io::copy(&mut file, zip_writer)?;
    } else if path.is_dir() {
        let relative_path = path.strip_prefix(base_path).unwrap_or(path);
        let folder_name = format!("{}/", relative_path.to_string_lossy());
        zip_writer.add_directory(folder_name, options)?;
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            add_path_to_zip(zip_writer, base_path, &entry.path(), options)?;
        }
    }
    Ok(())
}

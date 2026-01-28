use std::{
    collections::HashMap,
    ffi::OsStr,
    fs,
    io::{Cursor, Error, ErrorKind},
    net::{Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    sync::Arc,
};

use rocket::{
    fs::NamedFile,
    http::{Cookie, CookieJar, SameSite, Status},
    time::{Duration, OffsetDateTime},
};
use rocket_dyn_templates::tera::{to_value, try_get_value, Value};
use tokio::sync::RwLock;
use zip::write::SimpleFileOptions;

use crate::{FileEntry, HeaderFile, IndexResponse, MirrorFile, api::VideoFile, config::CONFIG};

pub fn read_dirs(path: &str) -> Result<Vec<MirrorFile>, Error> {
    let mut dir_list = Vec::new();

    let paths = match fs::read_dir(path) {
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
                    size: 0,
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

    let paths = match fs::read_dir(&path) {
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

            let mut icon = "folder";

            for subdir_path in subdir_paths {
                let subdir_path = subdir_path?;
                if let Some(file_name) = subdir_path.file_name().to_str() {
                    if file_name == "HIDDEN" {
                        continue 'main;
                    } else if file_name == "RESTRICTED" {
                        icon = "lockedfolder";
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
                .find(|entry| {
                    entry.file.strip_suffix("/").unwrap_or_default().to_string() == rel_path
                })
                .map(|entry| entry.size)
                .unwrap_or(0);

            if let Some(file_name) = file_path.file_name().to_str() {
                dir_list.push(MirrorFile {
                    name: file_name.to_string(),
                    ext: "folder".to_string(),
                    icon: icon.to_string(),
                    size: folder_size,
                    downloads: None,
                });
            }
        }
    }

    Ok(dir_list)
}

pub fn read_files(path: &str) -> Result<Vec<MirrorFile>, Error> {
    let mut file_list = Vec::new();

    let paths = match fs::read_dir(&path) {
        Ok(paths) => paths,
        Err(e) => return Err(e),
    };

    for file_path in paths {
        let file_path = file_path?;
        let md = fs::metadata(file_path.path())?;

        if md.is_file() {
            if let Some(file_name) = file_path.file_name().to_str() {
                let ext = get_extension_from_filename(file_name)
                    .unwrap_or_else(|| "")
                    .to_lowercase();

                let mut icon = ext.as_str();

                if !Path::new(&format!("public/static/images/icons/{}.png", &icon)).exists() {
                    icon = "default";
                }
                let file: MirrorFile = MirrorFile {
                    name: file_name.to_owned(),
                    ext: ext.to_string(),
                    icon: icon.to_string(),
                    size: md.len(),
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

pub fn get_theme<'a>(jar: &CookieJar<'_>) -> String {
    let mut theme = jar
        .get("theme")
        .map(|cookie| cookie.value())
        .unwrap_or("default");

    if !Path::new(&format!("public/static/styles/{}.css", &theme)).exists() {
        theme = "default";
    }

    theme.to_string()
}

pub fn is_restricted(path: &Path, is_logged_in: bool) -> bool {
    if !CONFIG.enable_login {
        return false;
    }
    let mut current = Some(path);

    while let Some(p) = current {
        if p.join("RESTRICTED").exists() {
            return !is_logged_in;
        }
        current = p.parent();
    }

    false
}

pub fn is_hidden(path: &Path, perms: Option<i32>) -> bool {
    let mut current = Some(path);

    while let Some(p) = current {
        if p.join("HIDDEN").exists() {
            if let Some(perms) = perms {
                return perms != 0;
            } else {
                return true;
            }
        }
        current = p.parent();
    }

    false
}

pub async fn open_file(path: PathBuf, cache_control: &str) -> Result<IndexResponse, Status> {
    if !path.exists() {
        return Err(Status::NotFound);
    }

    if CONFIG.standalone {
        match NamedFile::open(&path).await {
            Ok(f) => Ok(IndexResponse::NamedFile(f, cache_control.to_string())),
            Err(_) => Err(Status::InternalServerError),
        }
    } else {
        Ok(IndexResponse::HeaderFile(HeaderFile(
            path.display().to_string(),
            cache_control.to_string(),
        )))
    }
}

pub fn create_cookie<'a>(name: &'a str, value: &str) -> Cookie<'a> {
    let year = OffsetDateTime::now_utc() + Duration::days(365);
    let mut cookie = Cookie::new(name, value.to_string());
    cookie.set_expires(year);
    cookie.set_same_site(SameSite::Lax);
    cookie
}

pub fn parse_language(header: &str) -> Option<String> {
    let lang_dir = "lang/";

    for lang in header.split(',') {
        let code = lang.split(';').next()?.trim();
        let short_code = code.split('-').next()?.to_lowercase();

        if Path::new(&format!("{}/{}.toml", lang_dir, short_code)).exists() {
            return Some(short_code);
        }
    }

    None
}

pub fn get_root_domain<'a>(host: &str) -> String {
    if host.parse::<Ipv4Addr>().is_ok() || host.parse::<Ipv6Addr>().is_ok() {
        return CONFIG.fallback_root_domain.to_string();
    }

    return host
        .splitn(2, '.')
        .nth(1)
        .unwrap_or(&CONFIG.fallback_root_domain)
        .to_string();
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

pub fn map_io_error_to_status(e: Error) -> Status {
    if cfg!(debug_assertions) {
        eprintln!("IO Error: {}", e.kind());
    }
    match e.kind() {
        ErrorKind::NotFound => Status::NotFound,
        ErrorKind::PermissionDenied => Status::Forbidden,
        ErrorKind::StorageFull => Status::InsufficientStorage,
        _ => Status::InternalServerError,
    }
}

pub fn get_real_path(file: &PathBuf, username: String) -> Result<(PathBuf, bool), Status> {
    if let Ok(rest) = file.strip_prefix("private") {
        if username == "Nobody" {
            return Err(Status::Forbidden);
        }

        Ok((
            Path::new("files/")
                .join("private")
                .join(&username)
                .join(rest),
            true,
        ))
    } else {
        Ok((Path::new("files/").join(&file), false))
    }
}

pub fn get_real_path_with_perms(
    file: &PathBuf,
    username: String,
    perms: i32,
) -> Result<(PathBuf, bool), Status> {
    if let Ok(rest) = file.strip_prefix("private") {
        if username == "Nobody" {
            return Err(Status::Forbidden);
        }

        Ok((
            Path::new("files/")
                .join("private")
                .join(&username)
                .join(rest),
            true,
        ))
    } else {
        if perms != 0 {
            Err(Status::Forbidden)
        } else {
            Ok((Path::new("files/").join(&file), false))
        }
    }
}

pub fn parse_7z_output(output: &str) -> Vec<MirrorFile> {
    let mut files = Vec::new();

    for line in output.lines() {
        if let Some(_idx) = line.find("..") {
            let parts: Vec<&str> = line.split_whitespace().collect();

            if parts.len() < 5 {
                continue;
            }

            let filename = if let Ok(_) = parts[4].parse::<u64>() {
                parts[5..].join(" ")
            } else {
                parts[4..].join(" ")
            };

            let size: u64 = parts[3].parse().unwrap_or(0);

            if size == 0 {
                continue;
            }

            let name = filename.to_string();

            let ext = get_extension_from_filename(&filename)
                .unwrap_or_default()
                .to_string();

            let mut icon = ext.to_lowercase();

            if !Path::new(&format!("public/static/images/icons/{}.png", &icon)).exists() {
                icon = "default".to_string();
            }

            files.push(MirrorFile {
                name,
                ext: ext.to_string(),
                icon: icon.to_string(),
                size,
                downloads: None,
            });
        }
    }

    files
}

pub fn format_size(bytes: u64, use_si: bool) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }

    let k: f64 = if use_si { 1000.0 } else { 1024.0 };
    let sizes = if use_si {
        ["B", "KB", "MB", "GB", "TB"]
    } else {
        ["B", "KiB", "MiB", "GiB", "TiB"]
    };
    let bytes_f64 = bytes as f64;
    let i = (bytes_f64.ln() / k.ln()).floor() as usize;
    let value = bytes_f64 / k.powi(i as i32);

    format!("{:.1} {}", value, sizes[i])
}

pub fn format_size_filter(
    value: &Value,
    args: &HashMap<String, Value>,
) -> Result<Value, rocket_dyn_templates::tera::Error> {
    let num = try_get_value!("format_size", "value", u64, value);

    let use_si = match args.get("use_si") {
        Some(use_si) => try_get_value!("format_size", "use_si", bool, use_si),
        None => false,
    };

    Ok(to_value(format_size(num, use_si))
        .expect("json serializing should always be possible for a string"))
}

pub fn get_genre(genre: &str) -> Result<String, Status> {
    let toml_str = fs::read_to_string("genres.toml").map_err(map_io_error_to_status)?;
    let parsed: toml::Value = toml::from_str(&toml_str).map_err(|_| Status::InternalServerError)?;
    let genres: HashMap<String, String> = parsed
        .get("genres")
        .unwrap()
        .as_table()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();

    match genres.get(genre) {
        Some(genre) => Ok(genre.to_string()),
        None => Ok(genre.to_string()),
    }
}

pub fn get_cache_control(is_private: bool) -> String {
    if is_private {
        "private".into()
    } else if CONFIG.max_age == 0 {
        "public".into()
    } else {
        format!("public, max-age={}", CONFIG.max_age)
    }
}

pub fn get_static_cache_control() -> String {
    if CONFIG.static_max_age == 0 {
        "public".into()
    } else {
        format!("public, max-age={}", CONFIG.static_max_age)
    }
}

pub fn get_name_from_path(path: &PathBuf) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_str()
        .unwrap_or_default()
        .to_string()
}

pub fn get_extension_from_path(path: &PathBuf) -> String {
    path.extension()
        .unwrap_or_default()
        .to_str()
        .unwrap_or_default()
        .to_string()
}

pub fn get_virtual_path(path: &str) -> String {
    Path::new(&path.replace("files/", "/"))
        .display()
        .to_string()
}

pub fn is_hidden_path(path: &Path, perms: Option<i32>) -> bool {
    let mut current = Some(path);

    while let Some(p) = current {
        if CONFIG
            .hidden_files
            .contains(&get_name_from_path(&p.to_path_buf()))
        {
            if let Some(perms) = perms {
                return perms != 0;
            } else {
                return true;
            }
        }
        if Path::new("files/")
            .join(
                p.join("RESTRICTED")
                    .display()
                    .to_string()
                    .replacen("/", "", 1),
            )
            .exists()
        {
            return !perms.is_some();
        }
        current = p.parent();
    }

    false
}

pub fn is_hidden_path_str(path: &str, perms: Option<i32>) -> bool {
    is_hidden_path(Path::new(path), perms)
}

pub fn get_icon(file_name: &str) -> String {
    let ext = get_extension_from_filename(file_name)
        .unwrap_or_else(|| "")
        .to_lowercase();

    let mut icon = ext.as_str();

    if !Path::new(&format!("public/static/images/icons/{}.png", &icon)).exists() {
        icon = "default";
    }

    icon.to_string()
}

pub fn add_token_cookie<'a>(token: &str, host: &str, jar: &'a CookieJar<'_>) -> &'a CookieJar<'a> {
    let mut jwt_cookie = Cookie::new("matoken", token.to_string());
    jwt_cookie.set_domain(format!(".{}", get_root_domain(host)));
    jwt_cookie.set_same_site(SameSite::Lax);

    jar.add(jwt_cookie);

    let mut local_jwt_cookie = Cookie::new("token", token.to_string());
    local_jwt_cookie.set_same_site(SameSite::Lax);

    jar.add(local_jwt_cookie);

    jar
}

pub fn get_video_metadata(path: &str) -> VideoFile {
    let mdpath = format!("files/video/metadata{}.md", path.replace("video/", ""));
    let mdpath = Path::new(mdpath.as_str());

    let mut vidtitle = get_name_from_path(&Path::new(path).to_path_buf());

    let details = if mdpath.exists() {
        let markdown_text = fs::read_to_string(mdpath.display().to_string())
            .unwrap_or_else(|err| err.to_string());
        let mut lines = markdown_text.lines();

        vidtitle = lines
            .next()
            .unwrap_or("")
            .trim_start_matches('#')
            .trim()
            .to_string();
        let markdown = lines.collect::<Vec<&str>>().join("\n");

        Some(markdown::to_html(&markdown))
    } else {
        None
    };
    
    VideoFile { title: vidtitle, description: details }
}

pub fn parse_bool(input: &str) -> bool {
    match input.to_lowercase().as_str() {
        "true" => true,
        "false" => false,
        _ => false,
    }
}

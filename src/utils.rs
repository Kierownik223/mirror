use std::{
    collections::HashMap,
    ffi::OsStr,
    fs,
    io::Error,
    path::{Path, PathBuf},
};

use humansize::{format_size, DECIMAL};
use rocket::http::{Cookie, CookieJar};
use time::{Duration, OffsetDateTime};

use crate::{HeaderFile, MirrorFile};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct FolderSize {
    size: u64,
    file: String,
}

pub fn get_folder_sizes(json_file: &str) -> Result<HashMap<String, u64>, Error> {
    let json_content = fs::read_to_string(json_file).unwrap_or_default();
    let folder_sizes: Vec<FolderSize> = serde_json::from_str(&json_content).unwrap_or_default();

    let mut size_map = HashMap::new();
    for entry in folder_sizes {
        size_map.insert(entry.file, entry.size);
    }

    Ok(size_map)
}

pub fn read_dirs(path: &str) -> Result<Vec<MirrorFile>, Error> {
    let mut dir_list = Vec::new();
    let size_map = get_folder_sizes("sizes.json")?;

    let paths = match fs::read_dir("files".to_owned() + &path) {
        Ok(paths) => paths,
        Err(e) => return Err(e),
    };

    for file_path in paths {
        let file_path = file_path?;
        let md = fs::metadata(file_path.path())?;

        if md.is_dir() {
            let full_path = file_path.path();
            let relative_path = full_path
                .strip_prefix(std::env::current_dir()?)
                .unwrap_or(&full_path);

            if let Some(file_name) = file_path.file_name().to_str() {
                let folder_size = size_map
                    .get(&relative_path.display().to_string())
                    .cloned()
                    .unwrap_or(0);

                let file: MirrorFile = MirrorFile {
                    name: file_name.to_owned(),
                    ext: "folder".to_string(),
                    icon: "folder".to_string(),
                    size: format_size(folder_size, DECIMAL),
                };

                dir_list.push(file);
            }
        }
    }

    Ok(dir_list)
}

pub fn read_files(path: &str) -> Result<Vec<MirrorFile>, Error> {
    let mut dir_list = Vec::new();

    let paths = match fs::read_dir("files".to_owned() + &path) {
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
                };

                dir_list.push(file);
            }
        }
    }

    Ok(dir_list)
}

pub fn get_extension_from_filename(filename: &str) -> Option<&str> {
    Path::new(filename).extension().and_then(OsStr::to_str)
}

pub fn get_bool_cookie(jar: &CookieJar, key: &str) -> bool {
    match jar
        .get(key)
        .map(|cookie| cookie.value())
        .unwrap_or_else(|| "false")
    {
        "true" => true,
        "false" => false,
        _ => false,
    }
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
        .map(|cookie| cookie.value().to_string())
        .unwrap_or("standard".to_string());

    if !Path::new(&("files/static/styles/".to_owned() + &theme + ".css").to_string()).exists() {
        theme = "standard".to_string();
    }

    theme
}

pub fn k<T: 'static + Copy, U>(val: T) -> Box<dyn Fn(U) -> T> {
    Box::new(move |_| val)
}

pub fn is_logged_in<'r>(jar: &CookieJar<'_>) -> bool {
    jar.get_private("session").map(k(true)).unwrap_or(false)
}

pub fn is_restricted(mut path: PathBuf, jar: &CookieJar<'_>) -> bool {
    while let Some(parent) = path.parent() {
        if parent.join("RESTRICTED").exists() {
            if !is_logged_in(&jar) {
                return true;
            } else {
                return false;
            }
        }
        path = parent.to_path_buf();
    }
    false
}

pub fn open_file(path: PathBuf) -> Option<HeaderFile> {
    if path.exists() {
        return Some(HeaderFile(path.display().to_string()));
    } else {
        return None;
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

        let mut icon = &ext.as_str();
        if !Path::new(
            &("files/static/images/icons/".to_owned() + &icon.to_lowercase() + ".png").to_string(),
        )
        .exists()
        {
            icon = &"default";
        }

        let file: MirrorFile = MirrorFile {
            name: file.to_string(),
            ext: ext.to_string(),
            icon: icon.to_lowercase(),
            size: "---".to_string(),
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
            println!("{}", short_code);
            return Some(short_code);
        }
    }

    None
}
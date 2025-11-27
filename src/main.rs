use audiotags::{MimeType, Tag};
use db::{fetch_user, Db};
use rocket::data::ToByteUnit;
use rocket::http::{ContentType, Cookie, CookieJar, SameSite, Status};
use rocket::response::content::RawHtml;
use rocket::response::Redirect;
use rocket::State;
use rocket::{Data, Request};
use rocket_db_pools::{Connection, Database};
use rocket_multipart_form_data::{
    MultipartFormData, MultipartFormDataField, MultipartFormDataOptions, Repetition,
};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;
use tokio::time::sleep;
use utils::{
    create_cookie, get_bool_cookie, get_theme, is_restricted, open_file, parse_language, read_dirs,
    read_files,
};
use walkdir::WalkDir;

use rocket_dyn_templates::{context, Template};

use crate::config::CONFIG;
use crate::db::{add_download, FileDb};
use crate::guards::{FullUri, HeaderFile, Host, Settings, UsePlain, UseViewers};
use crate::i18n::{Language, TranslationStore};
use crate::jwt::JWT;
use crate::responders::{Cached, IndexResponse, IndexResult};
use crate::utils::{
    format_size_filter, get_cache_control, get_extension_from_path, get_genre, get_name_from_path,
    get_real_path, get_root_domain, is_hidden, map_io_error_to_status, parse_7z_output,
    read_dirs_async,
};

mod account;
mod admin;
mod api;
mod config;
mod db;
mod guards;
mod i18n;
mod jwt;
mod responders;
#[cfg(test)]
mod tests;
mod utils;

#[macro_use]
extern crate rocket;

#[derive(serde::Serialize, PartialOrd, serde::Deserialize)]
pub struct MirrorFile {
    name: String,
    ext: String,
    icon: String,
    size: u64,
    downloads: Option<i32>,
}

impl Eq for MirrorFile {}

impl PartialEq for MirrorFile {
    fn eq(&self, other: &Self) -> bool {
        (&self.name, &self.ext) == (&other.name, &other.ext)
    }
}

impl Ord for MirrorFile {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

#[derive(serde::Serialize)]
struct Disk {
    fs: String,
    used_space: u64,
    total_space: u64,
    used_space_readable: String,
    total_space_readable: String,
    mount_point: String,
}

#[derive(serde::Serialize)]
struct Sysinfo {
    total_mem: u64,
    total_mem_readable: String,
    used_mem: u64,
    used_mem_readable: String,
    disks: Vec<Disk>,
}

type FileSizes = Arc<RwLock<Vec<FileEntry>>>;

#[derive(Debug, Serialize, Clone)]
struct FileEntry {
    size: u64,
    file: String,
}

#[get("/poster/<file..>")]
async fn poster(
    file: PathBuf,
    token: Result<JWT, Status>,
    host: Host<'_>,
    jar: &CookieJar<'_>,
) -> Result<Result<Cached<(ContentType, Vec<u8>)>, Result<IndexResponse, Status>>, Status> {
    let username = if let Ok(token) = token {
        if let Some(t) = token.token {
            let mut jwt_cookie = Cookie::new("matoken", t.to_string());
            jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
            jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(jwt_cookie);

            let mut local_jwt_cookie = Cookie::new("token", t.to_string());
            local_jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(local_jwt_cookie);
        }

        token.claims.sub
    } else {
        "Nobody".into()
    };

    let (path, is_private) = if let Ok(rest) = file.strip_prefix("private") {
        if username == "Nobody" {
            if get_extension_from_path(&rest.to_path_buf()) == "mp3" {
                return Ok(Err(open_file(
                    Path::new(&"files/static/images/icons/256x256/mp3.png").to_path_buf(),
                    "private",
                )
                .await));
            }
            return Err(Status::Forbidden);
        }

        (
            Path::new("files/")
                .join("private")
                .join(&username)
                .join(rest),
            true,
        )
    } else {
        (Path::new("files/").join(&file), false)
    };

    if let Ok(tag) = Tag::new().read_from_path(&path) {
        if let Some(picture) = tag.album_cover() {
            let mime_type = match picture.mime_type {
                MimeType::Png => ("image", "png"),
                MimeType::Bmp => ("image", "bmp"),
                MimeType::Gif => ("image", "gif"),
                MimeType::Jpeg => ("image", "jpeg"),
                MimeType::Tiff => ("image", "tiff"),
            };
            return Ok(Ok(Cached {
                response: (
                    ContentType::new(mime_type.0, mime_type.1),
                    picture.data.to_vec(),
                ),
                header: if is_private { "private" } else { "public" },
            }));
        } else {
            return Ok(Err(open_file(
                Path::new(&"files/static/images/icons/256x256/mp3.png").to_path_buf(),
                if is_private { "private" } else { "public" },
            )
            .await));
        }
    } else {
        if !path.exists() {
            return Err(Status::NotFound);
        }

        let ext = if path.is_file() {
            path.extension().and_then(OsStr::to_str).unwrap_or("")
        } else {
            "folder"
        }
        .to_lowercase();

        let mut icon = format!("files/static/images/icons/256x256/{}.png", ext);

        if !Path::new(&(icon).to_string()).exists() {
            icon = "files/static/images/icons/256x256/default.png".to_string();
        }

        Ok(Err(open_file(
            Path::new(&icon).to_path_buf(),
            if is_private { "private" } else { "public" },
        )
        .await))
    }
}

#[get("/file/<file..>")]
async fn file(
    file: PathBuf,
    token: Result<JWT, Status>,
    host: Host<'_>,
    jar: &CookieJar<'_>,
) -> Result<IndexResponse, Status> {
    let username = if let Ok(token) = token.as_ref() {
        if let Some(t) = &token.token {
            let mut jwt_cookie = Cookie::new("matoken", t.to_string());
            jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
            jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(jwt_cookie);

            let mut local_jwt_cookie = Cookie::new("token", t.to_string());
            local_jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(local_jwt_cookie);
        }

        &token.claims.sub
    } else {
        &"Nobody".into()
    };

    let (path, is_private) = get_real_path(&file, username.to_string())?;

    if is_restricted(&path, token.is_ok()) {
        return Err(Status::Unauthorized);
    }

    open_file(path, &get_cache_control(is_private)).await
}

#[get("/<file..>?download")]
async fn download_with_counter(
    db: Connection<FileDb>,
    file: PathBuf,
    token: Result<JWT, Status>,
    host: Host<'_>,
    jar: &CookieJar<'_>,
) -> Result<IndexResponse, Status> {
    let username = if let Ok(token) = token.as_ref() {
        if let Some(t) = &token.token {
            let mut jwt_cookie = Cookie::new("matoken", t.to_string());
            jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
            jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(jwt_cookie);

            let mut local_jwt_cookie = Cookie::new("token", t.to_string());
            local_jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(local_jwt_cookie);
        }

        &token.claims.sub
    } else {
        &"Nobody".into()
    };

    let (path, is_private) = get_real_path(&file, username.to_string())?;

    if is_private {
        return open_file(path, "private").await;
    }

    let file = file.display().to_string();

    if !path.exists() {
        return Err(Status::NotFound);
    }

    if is_restricted(&path, token.is_ok()) {
        return Err(Status::Unauthorized);
    }

    let ext = if path.is_file() {
        path.extension().and_then(OsStr::to_str).unwrap_or("")
    } else {
        "folder"
    }
    .to_lowercase();

    if !CONFIG.extensions.contains(&ext) {
        return open_file(path, &get_cache_control(is_private)).await;
    } else if &ext == "folder" {
        return Err(Status::Forbidden);
    }

    add_download(db, &file).await;

    let url = format!("/file/{}", urlencoding::encode(&file)).replace("%2F", "/");

    return Ok(IndexResponse::Redirect(Redirect::found(url)));
}

#[get("/<file..>?download")]
async fn download(
    file: PathBuf,
    token: Result<JWT, Status>,
    host: Host<'_>,
    jar: &CookieJar<'_>,
) -> Result<IndexResponse, Status> {
    let username = if let Ok(token) = token.as_ref() {
        if let Some(t) = &token.token {
            let mut jwt_cookie = Cookie::new("matoken", t.to_string());
            jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
            jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(jwt_cookie);

            let mut local_jwt_cookie = Cookie::new("token", t.to_string());
            local_jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(local_jwt_cookie);
        }

        &token.claims.sub
    } else {
        &"Nobody".into()
    };

    let (path, is_private) = get_real_path(&file, username.to_string())?;

    if is_restricted(&path, token.is_ok()) {
        return Err(Status::Unauthorized);
    }

    open_file(path, &get_cache_control(is_private)).await
}

#[get("/<file..>", rank = 10)]
async fn index(
    file: PathBuf,
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    useplain: UsePlain<'_>,
    viewers: UseViewers<'_>,
    sizes: &State<FileSizes>,
    token: Result<JWT, Status>,
    uri: FullUri,
) -> IndexResult {
    let jwt = token.clone().unwrap_or_default();

    if let Some(t) = jwt.token {
        let mut jwt_cookie = Cookie::new("matoken", t.to_string());
        jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
        jwt_cookie.set_same_site(SameSite::Lax);

        jar.add(jwt_cookie);

        let mut local_jwt_cookie = Cookie::new("token", t.to_string());
        local_jwt_cookie.set_same_site(SameSite::Lax);

        jar.add(local_jwt_cookie);
    }

    let username = jwt.claims.sub;
    let perms = jwt.claims.perms;

    let path: PathBuf;
    let is_private: bool;

    let strings = translations.get_translation(&lang.0);

    let root_domain = get_root_domain(host.0);
    let theme = get_theme(jar);

    let hires = get_bool_cookie(jar, "hires", false);
    let smallhead = get_bool_cookie(jar, "smallhead", false);
    let use_si = get_bool_cookie(jar, "use_si", true);
    let audio_player = get_bool_cookie(jar, "audio_player", true);
    let video_player = get_bool_cookie(jar, "video_player", true);

    if let Ok((p, i)) = get_real_path(&file, username.clone()) {
        path = p;
        is_private = i;
    } else if let Err(e) = get_real_path(&file, username.clone()) {
        if e == Status::Forbidden {
            return Ok(IndexResponse::Template(Template::render(
                if *useplain.0 {
                    "plain/error/private"
                } else {
                    "error/private"
                },
                context! {
                    title: "/private",
                    lang,
                    strings,
                    root_domain,
                    host: host.0,
                    config: (*CONFIG).clone(),
                    theme,
                    is_logged_in: token.is_ok(),
                    admin: perms == 0,
                    hires,
                    smallhead,
                },
            )));
        } else {
            return Err(e);
        }
    } else {
        return Err(Status::UnprocessableEntity);
    }

    if !path.exists() {
        return Err(Status::NotFound);
    }

    if is_restricted(&path, token.is_ok()) {
        return Err(Status::Unauthorized);
    }

    let ext = if path.is_file() {
        path.extension().and_then(OsStr::to_str).unwrap_or("")
    } else {
        if !uri.0.ends_with("/") {
            return Ok(IndexResponse::Redirect(Redirect::moved(format!(
                "{}/",
                uri.0
            ))));
        }
        if is_private {
            "privatefolder"
        } else {
            "folder"
        }
    }
    .to_lowercase();

    let cache_control = &get_cache_control(is_private);

    match ext.as_str() {
        "md" => {
            let markdown_text = fs::read_to_string(&path).unwrap_or_else(|e| {
                format!(
                    "{} {:?}",
                    strings
                        .get("error_occured")
                        .unwrap_or(&"error_occured".to_string()),
                    e
                )
            });
            let markdown = markdown::to_html(&markdown_text);
            Ok(IndexResponse::Template(Template::render(
                if *useplain.0 { "plain/md" } else { "md" },
                context! {
                    title: format!("{} {}", strings.get("reading_markdown").unwrap_or(&("reading_markdown".into())), Path::new("/").join(&file).display()),
                    lang,
                    strings,
                    root_domain,
                    host: host.0,
                    config: (*CONFIG).clone(),
                    path: Path::new("/").join(&file).display().to_string(),
                    theme,
                    is_logged_in: token.is_ok(),
                    hires,
                    admin: perms == 0,
                    smallhead,
                    markdown
                },
            )))
        }
        "7z" | "rar" | "zip" => {
            if !*viewers.0 {
                return open_file(path, "private").await;
            }

            let output = Command::new("7z")
                .args(["l", &path.display().to_string()])
                .output()
                .map_err(map_io_error_to_status)?;

            let files = parse_7z_output(&String::from_utf8(output.stdout).unwrap_or_default());

            Ok(IndexResponse::Template(Template::render(
                if *useplain.0 { "plain/zip" } else { "zip" },
                context! {
                    title: format!("{} {}", strings.get("viewing_zip").unwrap_or(&("viewing_zip".into())), Path::new("/").join(&file).display()),
                    lang,
                    strings,
                    root_domain,
                    host: host.0,
                    config: (*CONFIG).clone(),
                    path: Path::new("/").join(&file).display().to_string(),
                    files,
                    theme,
                    is_logged_in: token.is_ok(),
                    username,
                    admin: perms == 0,
                    hires,
                    smallhead,
                    use_si,
                },
            )))
        }
        "mp4" => {
            if !*viewers.0 || !video_player {
                return open_file(path, "private").await;
            }

            let displaydetails = true;

            let videopath = Path::new("/").join(file.clone()).display().to_string();
            let videopath = videopath.as_str();

            let mdpath = format!("files/video/metadata{}.md", videopath.replace("video/", ""));
            let mdpath = Path::new(mdpath.as_str());

            let vidtitle = path.file_name();
            let vidtitle = vidtitle.unwrap_or_default().to_str();
            let mut vidtitle = vidtitle.unwrap_or("title").to_string();

            let details: String;

            if mdpath.exists() {
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

                details = markdown::to_html(&markdown);
            } else {
                details = strings
                    .get("no_details")
                    .unwrap_or(&("no_details".into()))
                    .to_string();
            }

            Ok(IndexResponse::Template(Template::render(
                if *useplain.0 { "plain/video" } else { "video" },
                context! {
                    title: format!("{} {}", strings.get("watching").unwrap_or(&("watching".into())), Path::new("/").join(file.clone()).display().to_string().as_str()),
                    lang,
                    strings,
                    root_domain,
                    host: host.0,
                    config: (*CONFIG).clone(),
                    path: videopath,
                    poster: format!("/images/videoposters{}.jpg", videopath.replace("video/", "")),
                    vidtitle,
                    theme,
                    is_logged_in: token.is_ok(),
                    username,
                    admin: perms == 0,
                    hires,
                    smallhead,
                    displaydetails,
                    details
                },
            )))
        }
        "mp3" | "m4a" | "m4b" | "flac" | "wav" => {
            if !*viewers.0 || !audio_player {
                return open_file(path, "private").await;
            }

            let audiopath = Path::new("/").join(file.clone()).display().to_string();
            let audiopath = audiopath.as_str();

            let generic_template = Template::render(
                if *useplain.0 { "plain/audio" } else { "audio" },
                context! {
                    title: format!("{} {}", strings.get("listening").unwrap_or(&("listening".into())), Path::new("/").join(file.clone()).display().to_string().as_str()),
                    lang: &lang,
                    strings,
                    root_domain: &root_domain,
                    host: host.0,
                    config: (*CONFIG).clone(),
                    path: audiopath,
                    audiotitle: get_name_from_path(&path),
                    theme: &theme,
                    is_logged_in: token.is_ok(),
                    username: &username,
                    admin: perms == 0,
                    hires,
                    smallhead,
                    artist: "N/A",
                    year: "N/A",
                    album: "N/A",
                    genre: "N/A",
                    track: None::<u16>,
                    cover: false
                },
            );

            if get_extension_from_path(&path) == "wav" {
                return Ok(IndexResponse::Template(generic_template));
            }

            if let Ok(tag) = Tag::new().read_from_path(&path) {
                let audiotitle = tag
                    .title()
                    .map(|s| s.to_string())
                    .unwrap_or(get_name_from_path(&path));

                let artist = tag.artist().map(|s| s.to_string());
                let album = tag.album_title().map(|s| s.to_string());
                let genre = tag.genre().map(|s| get_genre(s).unwrap_or(s.to_string()));
                let year = tag.year();
                let track = tag.track_number();

                let mut cover = false;

                if let Some(_picture) = tag.album_cover() {
                    cover = true;
                }

                Ok(IndexResponse::Template(Template::render(
                    if *useplain.0 { "plain/audio" } else { "audio" },
                    context! {
                        title: format!("{} {}", strings.get("listening").unwrap_or(&("listening".into())), Path::new("/").join(file.clone()).display().to_string().as_str()),
                        lang,
                        strings,
                        root_domain,
                        host: host.0,
                        config: (*CONFIG).clone(),
                        path: audiopath,
                        audiotitle,
                        theme,
                        is_logged_in: token.is_ok(),
                        username,
                        admin: perms == 0,
                        hires,
                        smallhead,
                        artist,
                        year,
                        album,
                        genre,
                        track,
                        cover
                    },
                )))
            } else {
                return Ok(IndexResponse::Template(generic_template));
            }
        }
        "folder" => {
            if is_hidden(
                &path,
                if let Ok(token) = token.clone() {
                    Some(token.claims.perms)
                } else {
                    None
                },
            ) {
                return Err(Status::NotFound);
            }

            let mut markdown = String::new();
            let path_str = Path::new("/").join(&file).display().to_string();

            let mut files =
                read_files(&path.display().to_string()).map_err(map_io_error_to_status)?;
            let mut dirs = read_dirs_async(&path.display().to_string(), sizes)
                .await
                .map_err(map_io_error_to_status)?;

            if files.iter().any(|f| f.name == "RESTRICTED") {
                for dir in dirs.iter_mut() {
                    dir.icon = "lockedfolder".to_string();
                }
            }

            if perms != 0 {
                dirs.retain(|x| !CONFIG.hidden_files.contains(&x.name));
                files.retain(|x| !CONFIG.hidden_files.contains(&x.name));
            }

            dirs.sort();
            files.sort();

            if files
                .iter()
                .any(|f| f.name == format!("README.{}.md", lang.0))
            {
                let md = fs::read_to_string(
                    Path::new(&("files".to_string() + &path_str))
                        .join(format!("README.{}.md", lang.0)),
                )
                .unwrap_or_default();
                markdown = markdown::to_html(&md);
            } else if files.iter().any(|f| f.name == "README.md") {
                let md = fs::read_to_string(
                    Path::new(&("files".to_string() + &path_str)).join("README.md"),
                )
                .unwrap_or_default();
                markdown = markdown::to_html(&md);
            }

            Ok(IndexResponse::Template(Template::render(
                if *useplain.0 { "plain/index" } else { "index" },
                context! {
                    title: &path_str,
                    lang,
                    strings,
                    root_domain,
                    host: host.0,
                    config: (*CONFIG).clone(),
                    path: &path_str,
                    dirs,
                    files,
                    theme,
                    is_logged_in: token.is_ok(),
                    username,
                    admin: perms == 0,
                    hires,
                    smallhead,
                    markdown,
                    private: is_private,
                    use_si,
                },
            )))
        }
        "privatefolder" => {
            let mut markdown = String::new();

            let mut files =
                read_files(&path.display().to_string()).map_err(map_io_error_to_status)?;
            let mut dirs = read_dirs_async(&path.display().to_string(), sizes)
                .await
                .map_err(map_io_error_to_status)?;

            dirs.sort();
            files.sort();

            if files
                .iter()
                .any(|f| f.name == format!("README.{}.md", lang.0))
            {
                let md = fs::read_to_string(
                    Path::new(&path.display().to_string()).join(format!("README.{}.md", lang.0)),
                )
                .unwrap_or_default();
                markdown = markdown::to_html(&md);
            } else if files.iter().any(|f| f.name == "README.md") {
                let md =
                    fs::read_to_string(Path::new(&path.display().to_string()).join("README.md"))
                        .unwrap_or_default();
                markdown = markdown::to_html(&md);
            }

            let path_str = if let Ok(rest) = file.strip_prefix("private") {
                if username.is_empty() {
                    return Err(Status::Forbidden);
                }

                Path::new("/").join(format!(
                    "private{}",
                    if rest.display().to_string() != String::new() {
                        format!("/{}", rest.display().to_string())
                    } else {
                        String::new()
                    }
                ))
            } else {
                Path::new("/").join(&file)
            }
            .display()
            .to_string();

            Ok(IndexResponse::Template(Template::render(
                if *useplain.0 { "plain/index" } else { "index" },
                context! {
                    title: &path_str,
                    lang,
                    strings,
                    root_domain,
                    host: host.0,
                    config: (*CONFIG).clone(),
                    path: &path_str,
                    dirs,
                    files,
                    theme,
                    is_logged_in: token.is_ok(),
                    username,
                    admin: perms == 0,
                    hires,
                    smallhead,
                    markdown,
                    private: is_private,
                    use_si,
                },
            )))
        }
        _ => {
            if CONFIG.extensions.contains(&ext) {
                Ok(IndexResponse::Template(Template::render(
                    if *useplain.0 {
                        "plain/details"
                    } else {
                        "details"
                    },
                    context! {
                        title: format!("{} {}", strings.get("file_details").unwrap_or(&("file_details".into())), Path::new("/").join(&file).display()),
                        lang,
                        strings,
                        root_domain,
                        host: host.0,
                        config: (*CONFIG).clone(),
                        path: Path::new("/").join(&file).display().to_string(),
                        theme,
                        is_logged_in: token.is_ok(),
                        username,
                        admin: perms == 0,
                        hires,
                        smallhead,
                        filename: get_name_from_path(&path),
                        filesize: fs::metadata(&path).unwrap().len(),
                        use_si,
                    },
                )))
            } else {
                open_file(path, cache_control).await
            }
        }
    }
}

#[get("/settings?<opt..>")]
fn settings(
    jar: &CookieJar<'_>,
    opt: Settings<'_>,
    lang: Language,
    translations: &State<TranslationStore>,
    host: Host<'_>,
    useplain: UsePlain<'_>,
    token: Result<JWT, Status>,
) -> IndexResponse {
    let (username, perms) = if let Ok(token) = token.as_ref() {
        if let Some(t) = &token.token {
            let mut jwt_cookie = Cookie::new("matoken", t.to_string());
            jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
            jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(jwt_cookie);

            let mut local_jwt_cookie = Cookie::new("token", t.to_string());
            local_jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(local_jwt_cookie);
        }

        (&token.claims.sub, &token.claims.perms)
    } else {
        (&"Nobody".into(), &1)
    };

    let mut lang = lang.0;
    let theme = get_theme(jar);
    let strings = translations.get_translation(&lang);

    let language_names = translations.available_languages();

    let settings_map = vec![
        ("hires", opt.hires),
        ("smallhead", opt.smallhead),
        ("plain", opt.plain),
        ("nooverride", opt.nooverride),
        ("viewers", opt.viewers),
        ("dir_browser", opt.dir_browser),
        ("use_si", opt.use_si),
        ("audio_player", opt.audio_player),
        ("video_player", opt.video_player),
    ];

    let mut redir = false;

    if let Some(theme_opt) = opt.theme {
        if Path::new(&format!("files/static/styles/{}.css", theme_opt)).exists() {
            jar.add(create_cookie("theme", &theme_opt));
            redir = true;
        } else {
            jar.add(create_cookie("theme", "default"));
        }
    }

    if !Path::new(&format!("lang/{}.toml", lang)).exists() {
        lang = "en".to_string();
    }

    if let Some(lang_opt) = opt.lang {
        if Path::new(&format!("lang/{}.toml", lang_opt)).exists() {
            jar.add(create_cookie("lang", &lang_opt));
            redir = true;
        } else {
            jar.add(create_cookie("lang", "en"));
        }
    }

    if let Some(lang) = opt.lang {
        jar.add(("lang", lang.to_string()));
        redir = true;
    }

    for (key, value) in settings_map {
        if let Some(val) = value {
            jar.add(create_cookie(key, val));
            if val == "true" {
                redir = true;
            }
        }
    }

    if redir {
        return IndexResponse::Redirect(Redirect::to(uri!("/")));
    }

    let show_cookie_notice = jar.iter().next().is_none();

    return IndexResponse::Template(Template::render(
        if *useplain.0 {
            "plain/settings"
        } else {
            "settings"
        },
        context! {
            title: strings.get("settings").unwrap_or(&("settings".into())),
            theme,
            lang,
            strings,
            root_domain: get_root_domain(host.0),
            host: host.0,
            config: (*CONFIG).clone(),
            is_logged_in: token.is_ok(),
            username,
            admin: *perms == 0,
            hires: get_bool_cookie(jar, "hires", false),
            smallhead: get_bool_cookie(jar, "smallhead", false),
            plain: *useplain.0,
            nooverride: get_bool_cookie(jar, "nooverride", false),
            viewers: get_bool_cookie(jar, "viewers", true),
            dir_browser: get_bool_cookie(jar, "dir_browser", true),
            use_si: get_bool_cookie(jar, "use_si", true),
            audio_player: get_bool_cookie(jar, "audio_player", true),
            video_player: get_bool_cookie(jar, "video_player", true),
            language_names,
            show_cookie_notice,
        },
    ));
}

#[get("/settings/fetch")]
async fn fetch_settings(
    db: Connection<Db>,
    jar: &CookieJar<'_>,
    lang: Language,
    translations: &State<TranslationStore>,
    host: Host<'_>,
    token: Result<JWT, Status>,
) -> Result<RawHtml<String>, Status> {
    let token = token?;

    if let Some(t) = token.token {
        let mut jwt_cookie = rocket::http::Cookie::new("matoken", t.to_string());
        jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
        jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(jwt_cookie);

        let mut local_jwt_cookie = rocket::http::Cookie::new("token", t.to_string());
        local_jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(local_jwt_cookie);
    }

    let strings = translations.get_translation(&lang.0);
    let username = token.claims.sub;

    if let Some(db_user) = fetch_user(db, username.as_str()).await {
        let decoded: HashMap<String, String> =
            serde_json::from_str(&db_user.mirror_settings.unwrap_or("{}".to_string()))
                .expect("Failed to parse JSON");

        for (key, value) in decoded {
            let mut now = OffsetDateTime::now_utc();
            now += Duration::days(365);
            let mut cookie = Cookie::new(key, value);
            cookie.set_expires(now);
            cookie.set_same_site(SameSite::Lax);
            jar.add(cookie);
        }
    }

    return Ok(RawHtml(format!(
        "<script>alert(\"{}\");window.location.replace(\"/settings\");</script>",
        strings
            .get("fetch_success")
            .unwrap_or(&("fetch_success".into()))
    )));
}

#[get("/settings/sync")]
async fn sync_settings(
    db: Connection<Db>,
    jar: &CookieJar<'_>,
    lang: Language,
    translations: &State<TranslationStore>,
    host: Host<'_>,
    token: Result<JWT, Status>,
) -> Result<RawHtml<String>, Status> {
    let token = token?;

    if let Some(t) = token.token {
        let mut jwt_cookie = rocket::http::Cookie::new("matoken", t.to_string());
        jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
        jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(jwt_cookie);

        let mut local_jwt_cookie = rocket::http::Cookie::new("token", t.to_string());
        local_jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(local_jwt_cookie);
    }

    let strings = translations.get_translation(&lang.0);
    let username = token.claims.sub;

    let keys = vec![
        "lang",
        "hires",
        "smallhead",
        "theme",
        "nooverride",
        "viewers",
        "use_si",
        "audio_player",
        "video_player",
    ];

    let mut cookie_map: HashMap<String, Option<String>> = HashMap::new();
    for key in keys {
        let value = jar.get(key).map(|cookie| cookie.value().to_string());
        cookie_map.insert(key.to_string(), value);
    }

    let settings = serde_json::to_string(&cookie_map).expect("Failed to serialize cookie data");

    db::update_settings(db, username.as_str(), settings.as_str()).await;

    return Ok(RawHtml(format!(
        "<script>alert(\"{}\");window.location.replace(\"/settings\");</script>",
        strings
            .get("sync_success")
            .unwrap_or(&("sync_success".into()))
    )));
}

#[get("/settings/reset")]
async fn reset_settings(jar: &CookieJar<'_>) -> Redirect {
    let keys = vec![
        "lang",
        "hires",
        "smallhead",
        "theme",
        "nooverride",
        "plain",
        "viewers",
        "use_si",
        "audio_player",
        "video_player",
    ];

    for key in keys {
        jar.remove(key);
    }

    return Redirect::to("/");
}

#[get("/iframe/<file..>")]
async fn iframe(
    file: PathBuf,
    jar: &CookieJar<'_>,
    host: Host<'_>,
    token: Result<JWT, Status>,
) -> Result<IndexResponse, Status> {
    let username = if let Ok(token) = token.as_ref() {
        if let Some(t) = &token.token {
            let mut jwt_cookie = Cookie::new("matoken", t.to_string());
            jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
            jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(jwt_cookie);

            let mut local_jwt_cookie = Cookie::new("token", t.to_string());
            local_jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(local_jwt_cookie);
        }

        &token.claims.sub
    } else {
        &"Nobody".into()
    };

    let path = get_real_path(&file, username.to_string())?.0;

    if is_restricted(&path, token.is_ok()) {
        return Err(Status::Unauthorized);
    }

    let path = get_real_path(&file, username.to_string())?
        .0
        .display()
        .to_string();

    let mut dirs = read_dirs(&path).map_err(map_io_error_to_status)?;

    dirs.retain(|x| !CONFIG.hidden_files.contains(&x.name));

    dirs.sort();

    Ok(IndexResponse::Template(Template::render(
        "iframe",
        context! {
            path: file.display().to_string(),
            dirs,
            theme: get_theme(jar),
            hires: get_bool_cookie(jar, "hires", false)
        },
    )))
}

#[get("/scripts/<file>?<lang>&<hires>")]
async fn scripts(
    file: &str,
    lang: Option<&str>,
    hires: Option<bool>,
    translations: &State<TranslationStore>,
    host: Host<'_>,
) -> Result<Cached<(ContentType, Template)>, Status> {
    let strings = translations.get_translation(lang.unwrap_or("en"));

    if !file.ends_with(".js") {
        return Err(Status::NotFound);
    }

    let file = file.trim_end_matches(".js");

    if !Path::new(&format!("templates/scripts/{}.js.tera", file)).exists() {
        return Err(Status::NotFound);
    }

    Ok(Cached {
        response: (
            ContentType::new("text", "javascript; charset=utf-8"),
            Template::render(
                format!("scripts/{}", file),
                context! {
                    config: (*CONFIG).clone(),
                    strings,
                    host: host.0,
                    hires: hires.unwrap_or(false),
                },
            ),
        ),
        header: "public, max-age=604800",
    })
}

#[get("/sitemap.xml")]
async fn sitemap(sizes: &State<FileSizes>, host: Host<'_>) -> Result<Cached<Template>, Status> {
    let files = sizes.read().await;
    let mut files = files.clone();

    files.retain(|file| {
        !CONFIG
            .hidden_files
            .iter()
            .any(|hidden| file.file.contains(hidden) || file.file.contains("private"))
    });

    for file in files.iter_mut() {
        file.file = file.file.strip_prefix("files").unwrap_or("").to_string();
    }

    files.retain(|file| !file.file.is_empty());

    let context = context! {
        files: files,
        host: host.0,
    };

    Ok(Cached {
        response: Template::render("sitemap", context),
        header: "public",
    })
}

#[get("/upload?<path>")]
fn uploader(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    useplain: UsePlain<'_>,
    token: Result<JWT, Status>,
    path: Option<&str>,
) -> Result<IndexResponse, Status> {
    let token = token?;

    if let Some(t) = token.token {
        let mut jwt_cookie = rocket::http::Cookie::new("matoken", t.to_string());
        jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
        jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(jwt_cookie);

        let mut local_jwt_cookie = rocket::http::Cookie::new("token", t.to_string());
        local_jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(local_jwt_cookie);
    }

    let username = token.claims.sub;
    let perms = token.claims.perms;

    let strings = translations.get_translation(&lang.0);

    return Ok(IndexResponse::Template(Template::render(
        if *useplain.0 {
            "plain/upload"
        } else {
            "upload"
        },
        context! {
            title: strings.get("uploader").unwrap_or(&("uploader".into())),
            lang,
            strings,
            root_domain: get_root_domain(host.0),
            host: host.0,
            config: (*CONFIG).clone(),
            theme: get_theme(jar),
            is_logged_in: true,
            hires: get_bool_cookie(jar, "hires", false),
            smallhead: get_bool_cookie(jar, "smallhead", false),
            username: username,
            admin: perms == 0,
            dir_browser: get_bool_cookie(jar, "dir_browser", true),
            path: path.unwrap_or_default(),
            uploadedfiles: vec![MirrorFile { name: "".to_string(), ext: "".to_string(), icon: "default".to_string(), size: 0, downloads: None }]
        },
    )));
}

#[post("/upload?<path>", data = "<data>")]
async fn upload(
    content_type: &ContentType,
    data: Data<'_>,
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    useplain: UsePlain<'_>,
    token: Result<JWT, Status>,
    path: Option<&str>,
) -> Result<IndexResponse, Status> {
    let token = token?;

    if let Some(t) = token.token {
        let mut jwt_cookie = rocket::http::Cookie::new("matoken", t.to_string());
        jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
        jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(jwt_cookie);

        let mut local_jwt_cookie = rocket::http::Cookie::new("token", t.to_string());
        local_jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(local_jwt_cookie);
    }

    let username = token.claims.sub;
    let perms = token.claims.perms;

    let options = MultipartFormDataOptions::with_multipart_form_data_fields(vec![
        MultipartFormDataField::file("files")
            .repetition(Repetition::infinite())
            .size_limit(u64::from(100.megabytes())),
        MultipartFormDataField::text("path"),
    ]);

    let form_data = match MultipartFormData::parse(content_type, data, options).await {
        Ok(data) => data,
        Err(err) => {
            eprintln!("Failed to parse multipart form data: {:?}", err);
            return Err(Status::BadRequest);
        }
    };

    let mut user_path = form_data
        .texts
        .get("path")
        .and_then(|paths| paths.first().map(|p| p.text.trim_matches('/').to_string()))
        .unwrap_or("uploads".to_string());

    if user_path.is_empty() {
        user_path = "uploads".to_string();
    }

    let is_private = user_path.starts_with("private");
    if !is_private && perms != 0 {
        return Err(Status::Forbidden);
    }

    let base_path = if is_private {
        format!(
            "files/private/{}/{}",
            username,
            user_path.trim_start_matches("private")
        )
    } else {
        format!("files/{}", user_path)
    };

    let mut uploaded_files: Vec<MirrorFile> = Vec::new();

    if let Some(file_fields) = form_data.files.get("files") {
        for file_field in file_fields {
            if let Some(file_name) = &file_field.file_name {
                let normalized_path = file_name.replace('\\', "/");
                let file_name = get_name_from_path(&Path::new(&normalized_path).to_path_buf());

                let upload_path = format!("{}/{}", base_path, file_name);

                match std::fs::File::create(&upload_path) {
                    Ok(mut file) => {
                        if let Ok(mut temp_file) = std::fs::File::open(&file_field.path) {
                            let mut buffer = Vec::new();
                            let _ = temp_file.read_to_end(&mut buffer);

                            let _ = file.write_all(&buffer);
                            let mut icon =
                                get_extension_from_path(&Path::new(&normalized_path).to_path_buf());
                            if !Path::new(&format!("files/static/images/icons/{}.png", &icon))
                                .exists()
                            {
                                icon = "default".to_string();
                            }

                            if perms == 0 {
                                uploaded_files.push(MirrorFile {
                                    name: file_name,
                                    ext: format!(
                                        "/{}/{}",
                                        user_path,
                                        get_name_from_path(
                                            &Path::new(&normalized_path).to_path_buf()
                                        )
                                    ),
                                    size: 0,
                                    icon: icon,
                                    downloads: None,
                                });
                            } else {
                                uploaded_files.push(MirrorFile {
                                    name: file_name,
                                    ext: format!(
                                        "/{}/{}",
                                        user_path.replacen(
                                            format!("/{}", &username).as_str(),
                                            "",
                                            1
                                        ),
                                        get_name_from_path(
                                            &Path::new(&normalized_path).to_path_buf()
                                        )
                                    ),
                                    size: 0,
                                    icon: icon,
                                    downloads: None,
                                });
                            }
                        } else {
                            eprintln!("Failed to open temp file for: {}", file_name);
                            return Err(Status::InternalServerError);
                        }
                    }
                    Err(err) => {
                        eprintln!("Failed to create target file {}: {:?}", upload_path, err);
                        continue;
                    }
                }
            } else {
                eprintln!("A file was uploaded without a name, skipping.");
                continue;
            }
        }

        let strings = translations.get_translation(&lang.0);

        return Ok(IndexResponse::Template(Template::render(
            if *useplain.0 {
                "plain/upload"
            } else {
                "upload"
            },
            context! {
                title: strings.get("uploader").unwrap_or(&("uploader".into())),
                lang,
                strings,
                root_domain: get_root_domain(host.0),
                host: host.0,
                config: (*CONFIG).clone(),
                theme: get_theme(jar),
                is_logged_in: true,
                hires: get_bool_cookie(jar, "hires", false),
                smallhead: get_bool_cookie(jar, "smallhead", false),
                username,
                admin: perms == 0,
                dir_browser: get_bool_cookie(jar, "dir_browser", true),
                path: path.unwrap_or_default(),
                uploadedfiles: uploaded_files
            },
        )));
    } else {
        return Err(Status::BadRequest);
    }
}

#[catch(422)]
async fn unprocessable_entry(_status: Status, req: &Request<'_>) -> Cached<(Status, Template)> {
    let jar = req.cookies();
    let translations = req.guard::<&State<TranslationStore>>().await.unwrap();
    let useplain = req
        .guard::<UsePlain<'_>>()
        .await
        .succeeded()
        .unwrap_or(UsePlain(&false));

    let mut lang = "en".to_string();

    if let Some(header) = req.headers().get_one("Accept-Language") {
        let header_lang = parse_language(header).unwrap_or("en".to_string());
        lang = header_lang;
    }

    if let Some(cookie_lang) = jar.get("lang").map(|c| c.value()) {
        lang = cookie_lang.to_string();
    }

    let strings = translations.get_translation(lang.as_str());

    let host = if let Some(host) = req.host() {
        &host.to_string()
    } else {
        &(*CONFIG).fallback_root_domain
    };

    Cached {
        response: (
            Status::BadRequest,
            Template::render(
                if *useplain.0 {
                    "plain/error/400"
                } else {
                    "error/400"
                },
                context! {
                    title: "HTTP 400",
                    lang,
                    strings,
                    root_domain: get_root_domain(&host),
                    host,
                    config: (*CONFIG).clone(),
                    theme: get_theme(jar),
                    is_logged_in: false,
                    admin: false,
                    hires: get_bool_cookie(jar, "hires", false),
                    smallhead: get_bool_cookie(jar, "smallhead", false),
                },
            ),
        ),
        header: "no-cache",
    }
}

#[catch(default)]
async fn default(status: Status, req: &Request<'_>) -> Cached<Template> {
    let jar = req.cookies();
    let translations = req.guard::<&State<TranslationStore>>().await.unwrap();
    let useplain = req
        .guard::<UsePlain<'_>>()
        .await
        .succeeded()
        .unwrap_or(UsePlain(&false));

    let mut lang = "en".to_string();

    if let Some(header) = req.headers().get_one("Accept-Language") {
        let header_lang = parse_language(header).unwrap_or("en".to_string());
        lang = header_lang;
    }

    if let Some(cookie_lang) = jar.get("lang").map(|c| c.value()) {
        lang = cookie_lang.to_string();
    }

    let strings = translations.get_translation(lang.as_str());

    let host = if let Some(host) = req.host() {
        &host.to_string()
    } else {
        &(*CONFIG).fallback_root_domain
    };

    Cached {
        response: Template::render(
            if *useplain.0 {
                format!("plain/error/{}", status.code)
            } else {
                format!("error/{}", status.code)
            },
            context! {
                title: format!("HTTP {}", status.code),
                lang,
                strings,
                root_domain: get_root_domain(&host),
                host,
                config: (*CONFIG).clone(),
                theme: get_theme(jar),
                is_logged_in: false,
                admin: false,
                hires: get_bool_cookie(jar, "hires", false),
                smallhead: get_bool_cookie(jar, "smallhead", false),
            },
        ),
        header: "no-cache",
    }
}

#[catch(401)]
fn forbidden(req: &Request) -> Redirect {
    Redirect::to(format!("/account/login?next={}", req.uri()))
}

async fn calculate_sizes(state: FileSizes) {
    loop {
        let mut file_sizes = Vec::new();
        let mut dir_sizes: HashMap<String, u64> = HashMap::new();

        for entry in WalkDir::new("files").into_iter().filter_map(Result::ok) {
            let path = entry.path().to_path_buf();
            if let Ok(metadata) = fs::metadata(&path) {
                let size = metadata.len();
                let path_str = path.display().to_string();

                if metadata.is_file() {
                    file_sizes.push(FileEntry {
                        size,
                        file: path_str,
                    });

                    let mut current = path.as_path();
                    while let Some(parent) = current.parent() {
                        let parent_str = parent.display().to_string();
                        *dir_sizes.entry(parent_str).or_insert(0) += size;
                        current = parent;
                    }
                }
            }
        }

        let mut all_entries = file_sizes;

        all_entries.extend(dir_sizes.into_iter().map(|(dir, size)| FileEntry {
            size,
            file: format!("{}/", dir),
        }));

        {
            let mut state_lock = state.write().await;
            *state_lock = all_entries;
        }

        sleep(tokio::time::Duration::from_secs(60)).await;
    }
}

#[launch]
#[tokio::main]
async fn rocket() -> _ {
    let size_state: FileSizes = Arc::new(RwLock::new(Vec::new()));

    let background_size_state = Arc::clone(&size_state);
    tokio::spawn(calculate_sizes(background_size_state));

    let mut rocket = rocket::build()
        .attach(Template::custom(|engine| {
            engine
                .tera
                .register_filter("format_size", format_size_filter);

            engine.tera.autoescape_on(vec![]);
        }))
        .manage(TranslationStore::new())
        .manage(size_state)
        .register("/", catchers![default, unprocessable_entry, forbidden])
        .mount(
            "/",
            routes![
                settings,
                reset_settings,
                index,
                iframe,
                poster,
                file,
                sitemap,
                uploader,
                upload,
                scripts
            ],
        );

    if CONFIG.enable_login {
        rocket = rocket
            .attach(account::build_account())
            .attach(admin::build())
            .attach(Db::init())
            .mount("/", routes![fetch_settings, sync_settings,]);
    }

    if CONFIG.enable_file_db {
        rocket = rocket
            .attach(FileDb::init())
            .mount("/", routes![download_with_counter])
    } else {
        rocket = rocket.mount("/", routes![download])
    }

    if CONFIG.enable_api {
        rocket = rocket.attach(api::build_api());
    }

    rocket
}

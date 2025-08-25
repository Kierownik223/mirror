use audiotags::{MimeType, Tag};
use db::{fetch_user, Db};
use humansize::{format_size, DECIMAL};
use rocket::fs::NamedFile;
use rocket::http::{ContentType, Cookie, CookieJar, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::response::content::RawHtml;
use rocket::response::{Redirect, Responder};
use rocket::{response, State};
use rocket::{Request, Response};
use rocket_db_pools::{Connection, Database};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;
use tokio::time::sleep;
use utils::{
    create_cookie, get_bool_cookie, get_session, get_theme, is_logged_in, is_restricted,
    list_to_files, open_file, parse_language, read_dirs, read_files,
};
use walkdir::WalkDir;

use rocket_dyn_templates::{context, Template};

use crate::db::{add_download, FileDb};
use crate::i18n::TranslationStore;
use crate::utils::{
    get_real_path, get_root_domain, is_hidden, map_io_error_to_status, parse_7z_output, read_dirs_async
};

mod account;
mod admin;
mod api;
mod db;
mod i18n;
mod utils;

#[derive(Debug, Deserialize, Clone, Serialize)]
struct Config {
    extensions: Vec<String>,
    hidden_files: Vec<String>,
    enable_login: bool,
    enable_api: bool,
    enable_marmak_link: bool,
    enable_direct: bool,
    instance_info: String,
    x_sendfile_header: String,
    x_sendfile_prefix: String,
    standalone: bool,
    fallback_root_domain: String,
    enable_file_db: bool,
}

impl Config {
    fn load() -> Self {
        let config_str = fs::read_to_string("config.toml").expect("Failed to read config file");
        toml::from_str(&config_str).expect("Failed to parse config file")
    }
}

#[macro_use]
extern crate rocket;

#[derive(serde::Serialize, PartialOrd, serde::Deserialize)]
pub struct MirrorFile {
    name: String,
    ext: String,
    icon: String,
    size: String,
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

#[derive(FromForm, serde::Serialize)]
struct Settings<'r> {
    theme: Option<&'r str>,
    lang: Option<&'r str>,
    hires: Option<&'r str>,
    smallhead: Option<&'r str>,
    plain: Option<&'r str>,
    nooverride: Option<&'r str>,
    viewers: Option<&'r str>,
    filebrowser: Option<&'r str>,
}

struct HeaderFile(String, bool);

impl<'r> Responder<'r, 'r> for HeaderFile {
    fn respond_to(self, _: &Request<'_>) -> response::Result<'r> {
        let config = Config::load();

        let mut builder = Response::build();

        builder.raw_header(
            config.x_sendfile_header,
            format!("{}{}", config.x_sendfile_prefix, self.0),
        );

        if self.1 {
            builder.raw_header("Cache-Control", "public");
        } else {
            builder.raw_header("Cache-Control", "private");
        }

        builder.ok()
    }
}

struct XForwardedFor<'r>(&'r str);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for XForwardedFor<'r> {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match request.headers().get_one("X-Forwarded-For") {
            Some(value) => {
                let mut ip = value.split(',').next().map(str::trim).unwrap_or(value);

                if ip == "127.0.0.1" || ip == "::1" {
                    ip = "(unknown)";
                }

                Outcome::Success(XForwardedFor(ip))
            }
            None => Outcome::Success(XForwardedFor("(unknown)")),
        }
    }
}
struct UsePlain<'r>(&'r bool);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for UsePlain<'r> {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match request.headers().get_one("User-Agent") {
            Some(value) => {
                if get_bool_cookie(request.cookies(), "plain", false) {
                    return Outcome::Success(UsePlain(&true));
                }

                if value.starts_with("Mozilla/1") || value.starts_with("Mozilla/2") {
                    return Outcome::Success(UsePlain(&true));
                }

                Outcome::Success(UsePlain(&false))
            }
            None => Outcome::Success(UsePlain(&true)),
        }
    }
}

struct UseViewers<'r>(&'r bool);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for UseViewers<'r> {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match request.headers().get_one("User-Agent") {
            Some(value) => {
                if value.starts_with("Winamp") || value.starts_with("VLC") {
                    return Outcome::Success(UseViewers(&false));
                }

                if get_bool_cookie(request.cookies(), "viewers", true) {
                    return Outcome::Success(UseViewers(&true));
                }

                Outcome::Success(UseViewers(&false))
            }
            None => Outcome::Success(UseViewers(&true)),
        }
    }
}

struct Host<'r>(&'r str);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Host<'r> {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match request.headers().get_one("Host") {
            Some(value) => Outcome::Success(Host(value)),
            None => Outcome::Error((Status::BadRequest, ())),
        }
    }
}

#[derive(serde::Serialize)]
struct Language(String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Language {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let cookies: &CookieJar = request.cookies();

        if let Some(cookie_lang) = cookies.get("lang").map(|c| c.value().to_string()) {
            return Outcome::Success(Language(cookie_lang));
        }

        if let Some(header_lang) = request.headers().get_one("Accept-Language") {
            if let Some(lang) = parse_language(header_lang) {
                return Outcome::Success(Language(lang));
            }
        }

        Outcome::Success(Language("en".to_string()))
    }
}

#[derive(Debug, PartialEq, Eq, FromForm)]
struct MarmakUser {
    username: String,
    password: String,
    perms: Option<i32>,
    mirror_settings: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct UserToken {
    username: String,
    password_hash: String,
}

#[derive(serde::Serialize)]
struct Disk {
    fs: String,
    used_space: u64,
    total_space: u64,
    used_space_readable: String,
    total_space_readable: String,
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

pub struct Cached<R> {
    response: R,
}

impl<'r, R: 'r + Responder<'r, 'static> + Send> Responder<'r, 'static> for Cached<R> {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        let mut res = self.response.respond_to(request)?;

        res.set_raw_header("Cache-Control", "public");

        Ok(res)
    }
}

enum IndexResponse {
    Template(Template),
    HeaderFile(HeaderFile),
    NamedFile(NamedFile),
    Redirect(Redirect),
}

type IndexResult = Result<IndexResponse, Status>;

#[rocket::async_trait]
impl<'r> Responder<'r, 'r> for IndexResponse {
    fn respond_to(self, req: &'r rocket::Request<'_>) -> rocket::response::Result<'r> {
        match self {
            IndexResponse::Template(t) => t.respond_to(req),
            IndexResponse::HeaderFile(h) => h.respond_to(req),
            IndexResponse::NamedFile(f) => f.respond_to(req),
            IndexResponse::Redirect(r) => r.respond_to(req),
        }
    }
}

#[get("/poster/<file..>")]
async fn poster(
    file: PathBuf,
    jar: &CookieJar<'_>,
) -> Result<Result<Cached<(ContentType, Vec<u8>)>, Result<IndexResponse, Status>>, Status> {
    let path = Path::new("files/").join(file);

    if is_restricted(&path, jar) {
        return Err(Status::Unauthorized);
    }

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
            }));
        } else {
            return Ok(Err(open_file(
                Path::new(&"files/static/images/icons/256x256/mp3.png").to_path_buf(),
                true,
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

        Ok(Err(open_file(Path::new(&icon).to_path_buf(), true).await))
    }
}

#[get("/file/<file..>")]
async fn file(file: PathBuf, jar: &CookieJar<'_>) -> Result<IndexResponse, Status> {
    let username = get_session(jar).0;
    let (path, is_private) = get_real_path(&file, username)?;

    if is_restricted(&path, jar) {
        return Err(Status::Unauthorized);
    }

    open_file(path, !is_private).await
}

#[get("/<file..>?download")]
async fn download_with_counter(
    db: Connection<FileDb>,
    file: PathBuf,
    jar: &CookieJar<'_>,
    config: &rocket::State<Config>,
) -> Result<IndexResponse, Status> {
    let username = get_session(jar).0;
    let (path, is_private) = get_real_path(&file, username)?;

    if is_private {
        return open_file(path, !is_private).await;
    }

    let file = file.display().to_string();

    if !path.exists() {
        return Err(Status::NotFound);
    }

    if is_restricted(&path, jar) {
        return Err(Status::Unauthorized);
    }

    let ext = if path.is_file() {
        path.extension().and_then(OsStr::to_str).unwrap_or("")
    } else {
        "folder"
    }
    .to_lowercase();

    if !config.extensions.contains(&ext) {
        return open_file(path, true).await;
    } else if &ext == "folder" {
        return Err(Status::Forbidden);
    }

    add_download(db, &file).await;

    let url = format!("/file/{}", urlencoding::encode(&file)).replace("%2F", "/");

    return Ok(IndexResponse::Redirect(Redirect::found(url)));
}

#[get("/<file..>?download")]
async fn download(file: PathBuf, jar: &CookieJar<'_>) -> Result<IndexResponse, Status> {
    let username = get_session(jar).0;
    let (path, is_private) = get_real_path(&file, username)?;

    if is_restricted(&path, jar) {
        return Err(Status::Unauthorized);
    }

    open_file(path, !is_private).await
}

#[get("/<file..>", rank = 10)]
async fn index(
    file: PathBuf,
    jar: &CookieJar<'_>,
    config: &rocket::State<Config>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    useplain: UsePlain<'_>,
    viewers: UseViewers<'_>,
    sizes: &State<FileSizes>,
) -> IndexResult {
    let (username, perms) = get_session(jar);
    let (path, is_private) = get_real_path(&file, username.clone())?;

    let strings = translations.get_translation(&lang.0);

    let root_domain = get_root_domain(host.0, &config.fallback_root_domain);
    let theme = get_theme(jar);

    let hires = get_bool_cookie(jar, "hires", false);
    let smallhead = get_bool_cookie(jar, "smallhead", false);

    if is_restricted(&path, jar) {
        return Err(Status::Unauthorized);
    }

    let ext = if path.is_file() {
        path.extension().and_then(OsStr::to_str).unwrap_or("")
    } else {
        if is_private {
            "privatefolder"
        } else {
            "folder"
        }
    }
    .to_lowercase();

    if !path.exists() {
        return Err(Status::NotFound);
    }

    match ext.as_str() {
        "md" => {
            let markdown_text = fs::read_to_string(&path).unwrap_or_default();
            let markdown = markdown::to_html(&markdown_text);
            Ok(IndexResponse::Template(Template::render(
                if *useplain.0 { "plain/md" } else { "md" },
                context! {
                    title: format!("{} {}", strings.get("reading_markdown").unwrap(), Path::new("/").join(&file).display()),
                    lang,
                    strings,
                    root_domain,
                    host: host.0,
                    config: config.inner(),
                    path: Path::new("/").join(&file).display().to_string(),
                    theme,
                    is_logged_in: is_logged_in(jar),
                    hires,
                    admin: perms == 0,
                    smallhead,
                    markdown
                },
            )))
        }
        "zip" => {
            if !*viewers.0 {
                return open_file(path, false).await;
            }

            let zip_file = fs::File::open(&path).map_err(|_| Status::BadRequest)?;
            if let Ok(archive) = zip::ZipArchive::new(zip_file) {
                let file_names: Vec<&str> = archive.file_names().collect();
                let files = list_to_files(file_names).unwrap_or_default();
                if files.is_empty() {
                    return Err(Status::NotFound);
                }

                Ok(IndexResponse::Template(Template::render(
                    if *useplain.0 { "plain/zip" } else { "zip" },
                    context! {
                        title: format!("{} {}", strings.get("viewing_zip").unwrap(), Path::new("/").join(&file).display()),
                        lang,
                        strings,
                        root_domain,
                        host: host.0,
                        config: config.inner(),
                        path: Path::new("/").join(&file).display().to_string(),
                        files,
                        theme,
                        is_logged_in: is_logged_in(jar),
                        username,
                        admin: perms == 0,
                        hires,
                        smallhead
                    },
                )))
            } else {
                open_file(path, false).await
            }
        }
        "7z" | "rar" => {
            if !*viewers.0 {
                return open_file(path, false).await;
            }

            let output = Command::new("7z")
                .args(["l", &path.display().to_string()])
                .output()
                .map_err(map_io_error_to_status)?;

            let files = parse_7z_output(&String::from_utf8(output.stdout).unwrap_or_default());

            Ok(IndexResponse::Template(Template::render(
                if *useplain.0 { "plain/zip" } else { "zip" },
                context! {
                    title: format!("{} {}", strings.get("viewing_zip").unwrap(), Path::new("/").join(&file).display()),
                    lang,
                    strings,
                    root_domain,
                    host: host.0,
                    config: config.inner(),
                    path: Path::new("/").join(&file).display().to_string(),
                    files,
                    theme,
                    is_logged_in: is_logged_in(jar),
                    username,
                    admin: perms == 0,
                    hires,
                    smallhead
                },
            )))
        }
        "mp4" => {
            if !*viewers.0 {
                return open_file(path, false).await;
            }

            let displaydetails = true;

            let videopath = Path::new("/").join(file.clone()).display().to_string();
            let videopath = videopath.as_str();

            let mdpath = format!("files/video/metadata{}.md", videopath.replace("video/", ""));
            let mdpath = Path::new(mdpath.as_str());

            let vidtitle = path.file_name();
            let vidtitle = vidtitle.unwrap().to_str();
            let mut vidtitle = vidtitle.unwrap().to_string();

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
                details = strings.get("no_details").unwrap().to_string();
            }

            Ok(IndexResponse::Template(Template::render(
                if *useplain.0 { "plain/video" } else { "video" },
                context! {
                    title: format!("{} {}", strings.get("watching").unwrap(), Path::new("/").join(file.clone()).display().to_string().as_str()),
                    lang,
                    strings,
                    root_domain,
                    host: host.0,
                    config: config.inner(),
                    path: videopath,
                    poster: format!("/images/videoposters{}.jpg", videopath.replace("video/", "")),
                    vidtitle,
                    theme,
                    is_logged_in: is_logged_in(&jar),
                    username,
                    admin: perms == 0,
                    hires,
                    smallhead,
                    displaydetails,
                    details
                },
            )))
        }
        "mp3" | "m4a" | "m4b" | "flac" => {
            if !*viewers.0 {
                return open_file(path, false).await;
            }

            let audiopath = Path::new("/").join(file.clone()).display().to_string();
            let audiopath = audiopath.as_str();

            if let Ok(tag) = Tag::new().read_from_path(&path) {
                let audiotitle = tag
                    .title()
                    .unwrap_or(&path.file_name().unwrap().to_str().unwrap());
                let artist = tag.artist().unwrap_or_default();
                let year = tag.year().unwrap_or(0);
                let album = tag.album_title().unwrap_or_default();
                let genre = tag.genre().unwrap_or_default();
                let track = tag.track_number().unwrap_or(0);

                let mut cover = false;

                if let Some(_picture) = tag.album_cover() {
                    cover = true;
                }

                Ok(IndexResponse::Template(Template::render(
                    if *useplain.0 { "plain/audio" } else { "audio" },
                    context! {
                        title: format!("{} {}", strings.get("listening").unwrap(), Path::new("/").join(file.clone()).display().to_string().as_str()),
                        lang,
                        strings,
                        root_domain,
                        host: host.0,
                        config: config.inner(),
                        path: audiopath,
                        audiotitle,
                        theme,
                        is_logged_in: is_logged_in(&jar),
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
                open_file(path, true).await
            }
        }
        "folder" => {
            if is_hidden(&path, jar) {
                return Err(Status::NotFound);
            }

            let mut markdown = String::new();
            let mut topmarkdown = false;
            let path_str = Path::new("/").join(&file).display().to_string();

            let mut files = read_files(&path_str).map_err(map_io_error_to_status)?;
            let mut dirs = read_dirs_async(&path_str, sizes)
                .await
                .map_err(map_io_error_to_status)?;

            if files.iter().any(|f| f.name == "top") {
                topmarkdown = true;
            }

            if files.iter().any(|f| f.name == "RESTRICTED") {
                for dir in dirs.iter_mut() {
                    dir.icon = "lockedfolder".to_string();
                }
            }

            dirs.retain(|x| !config.hidden_files.contains(&x.name));
            files.retain(|x| !config.hidden_files.contains(&x.name));

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
                    config: config.inner(),
                    path: &path_str,
                    dirs,
                    files,
                    theme,
                    is_logged_in: is_logged_in(jar),
                    username,
                    admin: perms == 0,
                    hires,
                    smallhead,
                    markdown,
                    topmarkdown,
                    filebrowser: !get_bool_cookie(jar, "filebrowser", false),
                    private: is_private,
                },
            )))
        }
        "privatefolder" => {
            let mut markdown = String::new();

            let mut path_str = if let Ok(rest) = file.strip_prefix("private") {
                if username.is_empty() {
                    return Err(Status::Forbidden);
                }

                Path::new("/").join("private").join(&username).join(rest)
            } else {
                Path::new("/").join(&file)
            }
            .display()
            .to_string();

            let mut files = read_files(&path_str).map_err(map_io_error_to_status)?;
            let mut dirs = read_dirs_async(&path_str, sizes)
                .await
                .map_err(map_io_error_to_status)?;

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

            path_str = if let Ok(rest) = file.strip_prefix("private") {
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
                    config: config.inner(),
                    path: &path_str,
                    dirs,
                    files,
                    theme,
                    is_logged_in: is_logged_in(jar),
                    username,
                    admin: perms == 0,
                    hires,
                    smallhead,
                    markdown,
                    filebrowser: !get_bool_cookie(jar, "filebrowser", false),
                    private: is_private,
                },
            )))
        }
        _ => {
            if config.extensions.contains(&ext) {
                Ok(IndexResponse::Template(Template::render(
                    if *useplain.0 {
                        "plain/details"
                    } else {
                        "details"
                    },
                    context! {
                        title: format!("{} {}", strings.get("file_details").unwrap(), Path::new("/").join(&file).display()),
                        lang,
                        strings,
                        root_domain,
                        host: host.0,
                        config: config.inner(),
                        path: Path::new("/").join(&file).display().to_string(),
                        theme,
                        is_logged_in: is_logged_in(jar),
                        username,
                        admin: perms == 0,
                        hires,
                        smallhead,
                        filename: path.file_name().unwrap().to_str(),
                        filesize: format_size(fs::metadata(&path).unwrap().len(), DECIMAL)
                    },
                )))
            } else {
                open_file(path, !is_private).await
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
    config: &State<Config>,
    useplain: UsePlain<'_>,
) -> Result<Template, Redirect> {
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
        ("filebrowser", opt.filebrowser),
    ];

    let mut redir = false;

    if let Some(theme_opt) = opt.theme {
        if Path::new(&format!("files/static/styles/{}.css", theme_opt)).exists() {
            jar.add(create_cookie("theme", &theme_opt));
            redir = true;
        } else {
            jar.add(create_cookie("theme", "standard"));
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
        return Err(Redirect::to(uri!("/")));
    }

    let show_cookie_notice = jar.iter().next().is_none();

    let username = if is_logged_in(jar) {
        get_session(jar).0
    } else {
        String::new()
    };

    return Ok(Template::render(
        if *useplain.0 {
            "plain/settings"
        } else {
            "settings"
        },
        context! {
            title: strings.get("settings").unwrap(),
            theme,
            lang,
            strings,
            root_domain: get_root_domain(host.0, &config.fallback_root_domain),
            host: host.0,
            config: config.inner(),
            is_logged_in: is_logged_in(jar),
            username,
            admin: get_session(jar).1 == 0,
            hires: get_bool_cookie(jar, "hires", false),
            smallhead: get_bool_cookie(jar, "smallhead", false),
            plain: *useplain.0,
            nooverride: get_bool_cookie(jar, "nooverride", false),
            viewers: get_bool_cookie(jar, "viewers", true),
            filebrowser: get_bool_cookie(jar, "filebrowser", false),
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
) -> Result<RawHtml<String>, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
        let strings = translations.get_translation(&lang.0);
        let username = get_session(jar).0;

        if let Some(db_user) = fetch_user(db, username.as_str()).await {
            let decoded: HashMap<String, String> =
                serde_json::from_str(&db_user.mirror_settings.unwrap_or("{}".to_string()))
                    .expect("Failed to parse JSON");

            for (key, value) in decoded {
                let mut now = OffsetDateTime::now_utc();
                now += Duration::days(365);
                let mut cookie = Cookie::new(key, value);
                cookie.set_expires(now);
                jar.add(cookie);
            }
        }

        return Ok(RawHtml(format!(
            "<script>alert(\"{}\");window.location.replace(\"/settings\");</script>",
            strings.get("fetch_success").unwrap()
        )));
    }
}

#[get("/settings/sync")]
async fn sync_settings(
    db: Connection<Db>,
    jar: &CookieJar<'_>,
    lang: Language,
    translations: &State<TranslationStore>,
) -> Result<RawHtml<String>, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
        let strings = translations.get_translation(&lang.0);
        let username = get_session(jar).0;

        let keys = vec![
            "lang",
            "hires",
            "smallhead",
            "theme",
            "nooverride",
            "viewers",
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
            strings.get("sync_success").unwrap()
        )));
    }
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
    config: &rocket::State<Config>,
) -> Result<Template, Status> {
    let username = get_session(jar).0;
    let path = get_real_path(&file, username.clone())?.0;

    if is_restricted(&path, jar) {
        return Err(Status::Unauthorized);
    }

    let path = if let Ok(rest) = file.strip_prefix("private") {
        if username.is_empty() {
            return Err(Status::Forbidden);
        }

        Path::new("/").join("private").join(&username).join(rest)
    } else {
        Path::new("/").join(&file)
    }
    .display()
    .to_string();

    let mut dirs = read_dirs(&path).map_err(map_io_error_to_status)?;

    dirs.retain(|x| !config.hidden_files.contains(&x.name));

    dirs.sort();

    Ok(Template::render(
        "iframe",
        context! {
            path,
            dirs,
            theme: get_theme(jar),
            hires: get_bool_cookie(jar, "hires", false)
        },
    ))
}

#[catch(422)]
fn unprocessable_entry() -> Status {
    Status::BadRequest
}

#[catch(default)]
async fn default(status: Status, req: &Request<'_>) -> Template {
    let jar = req.cookies();
    let translations = req.guard::<&State<TranslationStore>>().await.unwrap();
    let useplain = req.guard::<UsePlain<'_>>().await.unwrap();

    let mut lang = "en".to_string();

    if let Some(header) = req.headers().get_one("Accept-Language") {
        let header_lang = parse_language(header).unwrap_or("en".to_string());
        lang = header_lang;
    }

    if let Some(cookie_lang) = jar.get("lang").map(|c| c.value()) {
        lang = cookie_lang.to_string();
    }

    let strings = translations.get_translation(lang.as_str());

    let host = req.host().unwrap().to_string();

    let config = Config::load();

    Template::render(
        if *useplain.0 {
            format!("plain/error/{}", status.code)
        } else {
            format!("error/{}", status.code)
        },
        context! {
            title: format!("HTTP {}", status.code),
            lang,
            strings,
            root_domain: get_root_domain(&host, &config.fallback_root_domain),
            host,
            config: config,
            theme: get_theme(jar),
            is_logged_in: is_logged_in(jar),
            admin: get_session(jar).1 == 0,
            hires: get_bool_cookie(jar, "hires", false),
            smallhead: get_bool_cookie(jar, "smallhead", false),
        },
    )
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

        all_entries.extend(
            dir_sizes
                .into_iter()
                .map(|(dir, size)| FileEntry { size, file: dir }),
        );

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
    let config = Config::load();

    let size_state: FileSizes = Arc::new(RwLock::new(Vec::new()));

    let background_size_state = Arc::clone(&size_state);
    tokio::spawn(calculate_sizes(background_size_state));

    let mut rocket = rocket::build()
        .manage(config.clone())
        .attach(Template::fairing())
        .manage(TranslationStore::new())
        .manage(size_state)
        .register("/", catchers![default, unprocessable_entry, forbidden])
        .mount(
            "/",
            routes![settings, reset_settings, index, iframe, poster, file],
        );

    if config.enable_login {
        rocket = rocket
            .attach(account::build_account())
            .attach(admin::build())
            .attach(Db::init())
            .mount("/", routes![fetch_settings, sync_settings,]);
    }

    if config.enable_file_db {
        rocket = rocket
            .attach(FileDb::init())
            .mount("/", routes![download_with_counter])
    } else {
        rocket = rocket.mount("/", routes![download])
    }

    if config.enable_api {
        rocket = rocket.attach(api::build_api());
    }

    rocket
}

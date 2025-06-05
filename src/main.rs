use db::{fetch_user, Db};
use humansize::{format_size, DECIMAL};
use rocket::http::{Cookie, CookieJar, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::response::content::RawHtml;
use rocket::response::{Redirect, Responder};
use rocket::{response, State};
use rocket::{Request, Response};
use rocket_db_pools::{Connection, Database};
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use time::{Duration, OffsetDateTime};
use toml::Value;
use utils::{
    create_cookie, get_bool_cookie, get_session, get_theme, is_logged_in, is_restricted,
    list_to_files, open_file, parse_language, read_dirs, read_files,
};

use rocket_dyn_templates::{context, Template};

mod account;
mod admin;
mod api;
mod db;
mod utils;

#[derive(Debug, Deserialize, Clone)]
struct Config {
    extensions: Vec<String>,
    hidden_files: Vec<String>,
    enable_login: bool,
    enable_api: bool
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
    filebrowser: Option<&'r str>,
}

struct HeaderFile(String);

impl<'r> Responder<'r, 'r> for HeaderFile {
    fn respond_to(self, _: &Request) -> response::Result<'r> {
        Response::build().raw_header("X-Send-File", self.0).ok()
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
            None => Outcome::Error((Status::BadRequest, ())),
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
                cookies.add(Cookie::new("lang", lang.clone()));
                return Outcome::Success(Language(lang));
            }
        }

        cookies.add(Cookie::new("lang", "en"));
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

type Translations = HashMap<String, HashMap<String, String>>;

struct TranslationStore {
    translations: Translations,
}

impl TranslationStore {
    fn new() -> Self {
        let mut translations = HashMap::new();

        let lang_dir = Path::new("lang/");

        if let Ok(entries) = fs::read_dir(lang_dir) {
            for entry in entries.flatten() {
                if let Some(lang) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    if let Ok(contents) = fs::read_to_string(entry.path()) {
                        if let Ok(parsed) = contents.parse::<Value>() {
                            if let Some(table) = parsed.as_table() {
                                let lang_translations = table
                                    .iter()
                                    .filter_map(|(key, val)| {
                                        val.as_str().map(|s| (key.clone(), s.to_string()))
                                    })
                                    .collect();

                                println!("Loaded language {}", lang);

                                translations.insert(lang.to_string(), lang_translations);
                            }
                        }
                    }
                }
            }
        }

        Self { translations }
    }

    fn get_translation(&self, lang: &str) -> &HashMap<String, String> {
        if self.translations.contains_key(lang) {
            self.translations.get(lang).unwrap()
        } else {
            self.translations.get("en").unwrap()
        }
    }
}

#[get("/<file..>?download")]
async fn download(file: PathBuf, jar: &CookieJar<'_>) -> Result<Option<HeaderFile>, Status> {
    let path = Path::new("files/").join(file);

    if is_restricted(path.clone(), &jar) {
        return Err(Status::Forbidden);
    }

    Ok(open_file(path))
}

#[get("/<file..>")]
async fn index<'a>(
    file: PathBuf,
    jar: &CookieJar<'_>,
    config: &rocket::State<Config>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
) -> Result<Result<Result<Template, Redirect>, Option<HeaderFile>>, Status> {
    let path = Path::new("files/").join(file.clone());
    let strings = translations.get_translation(&lang.0);

    let root_domain = host.0.splitn(2, '.').nth(1).unwrap_or("marmak.net.pl");

    let (username, perms) = get_session(jar);

    let mut theme = get_theme(jar);

    let hires = get_bool_cookie(jar, "hires");
    let smallhead = get_bool_cookie(jar, "smallhead");
    let plain = get_bool_cookie(jar, "plain");

    if !Path::new(&("files/static/styles/".to_owned() + &theme + ".css").to_string()).exists() {
        theme = "standard".to_string();
    }

    if is_restricted(path.clone(), &jar) {
        return Err(Status::Forbidden);
    }

    let ext_upper = if path.is_file() {
        path.extension().and_then(OsStr::to_str).unwrap_or("")
    } else {
        "folder"
    };

    let ext = ext_upper.to_lowercase();

    match ext.as_str() {
        "md" => {
            if path.exists() {
                let markdown_text = fs::read_to_string(path.display().to_string())
                    .unwrap_or_else(|err| err.to_string());
                let markdown = markdown::to_html(&markdown_text);
                return Ok(Ok(Ok(Template::render(
                    "md",
                    context! {
                        title: format!("{} {}", strings.get("reading_markdown").unwrap(), Path::new("/").join(file.clone()).display()),
                        lang,
                        strings,
                        root_domain,
                        login: config.enable_login,
                        path: Path::new("/").join(file.clone()).display().to_string(),
                        theme: theme,
                        is_logged_in: is_logged_in(&jar),
                        hires: hires,
                        admin: perms == 0,
                        smallhead: smallhead,
                        markdown: markdown
                    },
                ))));
            } else {
                return Err(Status::NotFound);
            }
        }
        "zip" => {
            if path.exists() {
                let zip_file = fs::File::open(path.display().to_string()).unwrap();

                let archive = zip::ZipArchive::new(zip_file).unwrap();

                let file_names: Vec<&str> = archive.file_names().collect();

                let file_list = list_to_files(file_names).unwrap_or_default();

                if file_list.is_empty() {
                    return Err(Status::NotFound);
                }

                Ok(Ok(Ok(Template::render(
                    "zip",
                    context! {
                        title: format!("{} {}", strings.get("viewing_zip").unwrap(), Path::new("/").join(file.clone()).display().to_string().as_str()),
                        lang,
                        strings,
                        root_domain,
                        login: config.enable_login,
                        path: Path::new("/").join(file.clone()).display().to_string(),
                        files: file_list,
                        theme: theme,
                        is_logged_in: is_logged_in(&jar),
                        username: username,
                        admin: perms == 0,
                        hires: hires,
                        smallhead: smallhead
                    },
                ))))
            } else {
                return Err(Status::NotFound);
            }
        }
        "mp4" => {
            if path.exists() {
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

                Ok(Ok(Ok(Template::render(
                    "video",
                    context! {
                        title: format!("{} {}", strings.get("watching").unwrap(), Path::new("/").join(file.clone()).display().to_string().as_str()),
                        lang,
                        strings,
                        root_domain,
                        login: config.enable_login,
                        path: videopath,
                        poster: format!("/images/videoposters{}.jpg", videopath.replace("video/", "")),
                        vidtitle: vidtitle,
                        theme: theme,
                        is_logged_in: is_logged_in(&jar),
                        username: username,
                        admin: perms == 0,
                        hires: hires,
                        smallhead: smallhead,
                        displaydetails: displaydetails,
                        details: details
                    },
                ))))
            } else {
                return Err(Status::NotFound);
            }
        }
        "folder" => {
            let mut notroot = true;
            let mut markdown: String = "".to_string();
            let mut topmarkdown = false;
            let path = Path::new("/").join(file).display().to_string();
            let path_seg: Vec<&str> = path.split("/").collect();

            if path == "/" {
                notroot = false;
            }

            let mut file_list = read_files(&path).unwrap_or_default();
            let mut dir_list = read_dirs(&path).unwrap_or_default();

            if dir_list.is_empty() && file_list.is_empty() {
                return Err(Status::NotFound);
            }

            if file_list.contains(&MirrorFile {
                name: "top".to_owned(),
                ext: String::new(),
                icon: "default".to_string(),
                size: String::new(),
            }) {
                topmarkdown = true;
            }

            if file_list.contains(&MirrorFile {
                name: "RESTRICTED".to_owned(),
                ext: String::new(),
                icon: "default".to_string(),
                size: String::new(),
            }) {
                for dir in dir_list.iter_mut() {
                    dir.icon = "lockedfolder".to_string();
                }
            }

            dir_list.retain(|x| !config.hidden_files.contains(&x.name));
            file_list.retain(|x| !config.hidden_files.contains(&x.name));

            dir_list.sort();
            file_list.sort();

            if file_list.contains(&MirrorFile {
                name: format!("README.{}.md", lang.0),
                ext: "md".to_string(),
                icon: "default".to_string(),
                size: String::new(),
            }) {
                let markdown_text = fs::read_to_string(
                    Path::new(&("files".to_string() + &path))
                        .join(format!("README.{}.md", lang.0))
                        .display()
                        .to_string(),
                )
                .unwrap_or_else(|err| err.to_string());
                markdown = markdown::to_html(&markdown_text);
            } else if file_list.contains(&MirrorFile {
                name: "README.md".to_owned(),
                ext: "md".to_string(),
                icon: "default".to_string(),
                size: String::new(),
            }) {
                let markdown_text = fs::read_to_string(
                    Path::new(&("files".to_string() + &path))
                        .join("README.md")
                        .display()
                        .to_string(),
                )
                .unwrap_or_else(|err| err.to_string());
                markdown = markdown::to_html(&markdown_text);
            }

            if plain {
                return Ok(Ok(Ok(Template::render(
                    "plain",
                    context! {
                        title: path.to_string(),
                        lang,
                        strings,
                        root_domain,
                        login: config.enable_login,
                        path_seg: path_seg,
                        dirs: dir_list,
                        files: file_list,
                        notroot: notroot,
                        markdown: markdown,
                        topmarkdown: topmarkdown
                    },
                ))));
            }

            Ok(Ok(Ok(Template::render(
                "index",
                context! {
                    title: path.to_string(),
                    lang,
                    strings,
                    root_domain,
                    login: config.enable_login,
                    path_seg: path_seg,
                    dirs: dir_list,
                    files: file_list,
                    theme: theme,
                    is_logged_in: is_logged_in(&jar),
                    username: username,
                    admin: perms == 0,
                    hires: hires,
                    notroot: notroot,
                    smallhead: smallhead,
                    markdown: markdown,
                    topmarkdown: topmarkdown,
                    filebrowser: !get_bool_cookie(jar, "filebrowser"),
                },
            ))))
        }
        _ => {
            if config.extensions.contains(&ext) {
                if path.exists() {
                    return Ok(Ok(Ok(Template::render(
                        "details",
                        context! {
                            title: format!("{} {}", strings.get("file_details").unwrap(), Path::new("/").join(file.clone()).display().to_string().as_str()),
                            lang,
                            strings,
                            root_domain,
                            login: config.enable_login,
                            path: Path::new("/").join(file.clone()).display().to_string(),
                            theme: theme,
                            is_logged_in: is_logged_in(&jar),
                            username: username,
                            admin: perms == 0,
                            hires: hires,
                            smallhead: smallhead,
                            filename: path.file_name().unwrap().to_str(),
                            filesize: format_size(fs::metadata(path.clone()).unwrap().len(), DECIMAL)
                        },
                    ))));
                } else {
                    return Err(Status::NotFound);
                }
            } else {
                return Ok(Err(open_file(path)));
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
    config: &State<Config>
) -> Result<Template, Redirect> {
    let mut lang = lang.0;
    let mut theme = get_theme(jar);
    let strings = translations.get_translation(&lang);

    let root_domain = host.0.splitn(2, '.').nth(1).unwrap_or("marmak.net.pl");

    let settings_map = vec![
        ("hires", opt.hires),
        ("smallhead", opt.smallhead),
        ("plain", opt.plain),
        ("nooverride", opt.nooverride),
        ("filebrowser", opt.filebrowser),
    ];

    let mut redir = false;

    if !Path::new(&format!("files/static/styles/{}.css", theme)).exists() {
        theme = "standard".to_string();
    }

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

    let username = if is_logged_in(&jar) {
        get_session(jar).0
    } else {
        String::new()
    };

    Ok(Template::render(
        "settings",
        context! {
            title: strings.get("settings").unwrap(),
            theme,
            lang,
            strings,
            root_domain,
            login: config.enable_login,
            is_logged_in: is_logged_in(&jar),
            username,
            admin: get_session(jar).1 == 0,
            hires: get_bool_cookie(jar, "hires"),
            smallhead: get_bool_cookie(jar, "smallhead"),
            plain: get_bool_cookie(jar, "plain"),
            nooverride: get_bool_cookie(jar, "nooverride"),
            filebrowser: get_bool_cookie(jar, "filebrowser")
        },
    ))
}

#[get("/settings/fetch")]
async fn fetch_settings(
    db: Connection<Db>,
    jar: &CookieJar<'_>,
    lang: Language,
    translations: &State<TranslationStore>,
) -> Result<RawHtml<String>, Status> {
    if is_logged_in(&jar) {
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
    } else {
        return Err(Status::Forbidden);
    }
}

#[get("/settings/sync")]
async fn sync_settings(
    db: Connection<Db>,
    jar: &CookieJar<'_>,
    lang: Language,
    translations: &State<TranslationStore>,
) -> Result<RawHtml<String>, Status> {
    if is_logged_in(&jar) {
        let strings = translations.get_translation(&lang.0);
        let username = get_session(jar).0;

        let keys = vec![
            "lang",
            "hires",
            "smallhead",
            "theme",
            "nooverride",
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
    } else {
        return Err(Status::Forbidden);
    }
}

#[get("/iframe/<file..>")]
async fn iframe(
    file: PathBuf,
    jar: &CookieJar<'_>,
    config: &rocket::State<Config>,
) -> Result<Template, Status> {
    let path = Path::new("files/").join(file.clone());

    if is_restricted(path.clone(), &jar) {
        return Err(Status::Forbidden);
    }

    let mut notroot = true;

    let path = Path::new("/").join(file).display().to_string();

    if path == "/" {
        notroot = false;
    }

    let mut dir_list = read_dirs(&path).unwrap_or_default();
    let file_list = read_files(&path).unwrap_or_default();

    if dir_list.is_empty() && file_list.is_empty() {
        return Err(Status::NotFound);
    }

    dir_list.retain(|x| !config.hidden_files.contains(&x.name));

    dir_list.sort();

    Ok(Template::render(
        "iframe",
        context! {
            path: path,
            dirs: dir_list,
            theme: get_theme(jar),
            hires: get_bool_cookie(jar, "hires"),
            notroot: notroot
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

    let root_domain = host.splitn(2, '.').nth(1).unwrap_or("marmak.net.pl");

    Template::render(
        format!("error/{}", status.code),
        context! {
            title: format!("HTTP {}", status.code),
            lang,
            strings,
            root_domain,
            theme: get_theme(jar),
            is_logged_in: is_logged_in(&jar),
            admin: get_session(&jar).1 == 0,
            hires: get_bool_cookie(jar, "hires"),
            smallhead: get_bool_cookie(jar, "smallhead"),
        },
    )
}

#[catch(403)]
fn forbidden(req: &Request) -> Redirect {
    Redirect::to(format!("/account/login?next={}", req.uri()))
}

#[launch]
fn rocket() -> _ {
    let config = Config::load();

    let mut rocket = rocket::build()
        .manage(config.clone())
        .attach(Template::fairing())
        .manage(TranslationStore::new())
        .register("/", catchers![default, unprocessable_entry, forbidden])
        .mount(
            "/",
            routes![
                settings,
                download,
                index,
                iframe
            ],
        );

    if config.enable_login {
        rocket = rocket
            .attach(account::build_account())
            .attach(admin::build())
            .attach(Db::init())
            .mount("/", routes![
                fetch_settings,
                sync_settings,
            ]);
    }

    if config.enable_api {
        rocket = rocket
            .attach(api::build_api());
    }

    rocket
}

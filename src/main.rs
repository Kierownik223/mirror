use ::sysinfo::{Disks, System};
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use db::{fetch_user, login_user, Db};
use humansize::{format_size, DECIMAL};
use openssl::rsa::{Padding, Rsa};
use rocket::data::ToByteUnit;
use rocket::form::Form;
use rocket::http::{ContentType, Cookie, CookieJar, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::response;
use rocket::response::content::RawHtml;
use rocket::response::{Redirect, Responder};
use rocket::{Data, Request, Response};
use rocket_db_pools::{Connection, Database};
use rocket_multipart_form_data::{
    MultipartFormData, MultipartFormDataField, MultipartFormDataOptions, Repetition,
};
use serde_json::json;
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use time::{Duration, OffsetDateTime};
use utils::{
    create_cookie, get_bool_cookie, get_extension_from_filename, get_session, get_theme,
    is_logged_in, is_restricted, list_to_files, open_file, read_dirs, read_files,
};

use rocket_dyn_templates::{context, Template};

mod api;
mod db;
mod utils;


#[derive(Debug, Deserialize)]
struct Config {
    extensions: Vec<String>,
    hidden_files: Vec<String>,
}

impl Config {
    fn load() -> Self {
        let config_str = fs::read_to_string("config.toml")
            .expect("Failed to read config file");
        toml::from_str(&config_str)
            .expect("Failed to parse config file")
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
    nolang: Option<&'r str>,
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
        match request.headers().get_one("Cf-Connecting-Ip") {
            Some(value) => Outcome::Success(XForwardedFor(value)),
            None => Outcome::Error((Status::BadRequest, ())),
        }
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

#[get("/<file..>?download")]
async fn download(file: PathBuf, jar: &CookieJar<'_>) -> Result<Option<HeaderFile>, Status> {
    let path = Path::new("files/").join(file);

    if is_restricted(path.clone(), &jar) {
        return Err(Status::Forbidden);
    }

    Ok(open_file(path))
}

#[get("/<file..>")]
async fn index<'a>(file: PathBuf, jar: &CookieJar<'_>, config: &rocket::State<Config>) -> Result<Result<Result<Template, Redirect>, Option<HeaderFile>>, Status> {
    let path = Path::new("files/").join(file.clone());

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

    let ext_upper = path.extension().and_then(OsStr::to_str).unwrap_or("");

    let ext = ext_upper.to_lowercase();

    if ext == "md" {
        if path.exists() {
            let markdown_text = fs::read_to_string(path.display().to_string())
                .unwrap_or_else(|err| err.to_string());
            let markdown = markdown::to_html(&markdown_text);
            return Ok(Ok(Ok(Template::render(
                "md",
                context! {
                    title: format!("Reading file {}", Path::new("/").join(file.clone()).display()),
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
    } else if ext == "zip" {
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
                    title: format!("Viewing ZIP file {}", Path::new("/").join(file.clone()).display().to_string().as_str()),
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
    } else if ext == "mp4" {
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
                details = "No details available!".to_string();
            }

            Ok(Ok(Ok(Template::render(
                "video",
                context! {
                    title: format!("Watching {}", Path::new("/").join(file.clone()).display().to_string().as_str()),
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
    } else if ext == "" {
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
    } else if config.extensions.contains(&ext) {
        if path.exists() {
            return Ok(Ok(Ok(Template::render(
                "details",
                context! {
                    title: format!("Details of file {}", Path::new("/").join(file.clone()).display().to_string().as_str()),
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

#[get("/settings?<opt..>")]
fn settings(jar: &CookieJar<'_>, opt: Settings<'_>) -> Result<Template, Redirect> {
    let mut theme = get_theme(jar);

    let settings_map = vec![
        ("hires", opt.hires),
        ("smallhead", opt.smallhead),
        ("nolang", opt.nolang),
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
            title: "Settings",
            theme,
            is_logged_in: is_logged_in(&jar),
            username,
            admin: get_session(jar).1 == 0,
            hires: get_bool_cookie(jar, "hires"),
            smallhead: get_bool_cookie(jar, "smallhead"),
            nolang: get_bool_cookie(jar, "nolang"),
            plain: get_bool_cookie(jar, "plain"),
            nooverride: get_bool_cookie(jar, "nooverride"),
            filebrowser: get_bool_cookie(jar, "filebrowser")
        },
    ))
}

#[post("/upload", data = "<data>")]
async fn upload(
    content_type: &ContentType,
    data: Data<'_>,
    jar: &CookieJar<'_>,
) -> Result<Template, Status> {
    if is_logged_in(&jar) {
        let (username, perms) = get_session(jar);

        if perms != 0 {
            return Err(Status::Forbidden);
        }

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

        let mut uploaded_files: Vec<MirrorFile> = Vec::new();

        if let Some(file_fields) = form_data.files.get("files") {
            for file_field in file_fields {
                if let Some(file_name) = &file_field.file_name {
                    let upload_path = format!("files/{}/{}", user_path, file_name);

                    match std::fs::File::create(&upload_path) {
                        Ok(mut file) => {
                            if let Ok(mut temp_file) = std::fs::File::open(&file_field.path) {
                                let mut buffer = Vec::new();
                                let _ = temp_file.read_to_end(&mut buffer);

                                let _ = file.write_all(&buffer);
                                let mut icon = get_extension_from_filename(file_name)
                                    .unwrap_or("")
                                    .to_string()
                                    .to_lowercase();
                                if !Path::new(
                                    &("files/static/images/icons/".to_owned() + &icon + ".png")
                                        .to_string(),
                                )
                                .exists()
                                {
                                    icon = "default".to_string();
                                }
                                uploaded_files.push(MirrorFile {
                                    name: file_name.to_string(),
                                    ext: format!("/{}/{}", user_path, file_name),
                                    size: String::new(),
                                    icon: icon,
                                });
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

            return Ok(Template::render(
                "upload",
                context! {
                    title: "File uploader",
                    theme: get_theme(jar),
                    is_logged_in: is_logged_in(&jar),
                    hires: get_bool_cookie(jar, "hires"),
                    smallhead: get_bool_cookie(jar, "smallhead"),
                    username: username,
                    admin: perms == 0,
                    filebrowser: !get_bool_cookie(jar, "filebrowser"),
                    uploadedfiles: uploaded_files
                },
            ));
        } else {
            return Err(Status::BadRequest);
        }
    } else {
        return Err(Status::Forbidden);
    }
}

#[get("/sysinfo")]
fn sysinfo(jar: &CookieJar<'_>) -> Result<Template, Status> {
    if is_logged_in(&jar) {
        let username = get_session(jar).0;

        let mut sys = System::new_all();

        sys.refresh_all();

        let total_mem = sys.total_memory();
        let used_mem = sys.used_memory();

        let sys_name = System::name().unwrap_or(String::from("MARMAK Mirror"));
        let sys_ver = System::kernel_version().unwrap_or(String::from("21.3.7"));
        let hostname = System::host_name().unwrap_or(String::from("mirror"));

        let mut disks: Vec<Disk> = Vec::new();

        let sys_disks = Disks::new_with_refreshed_list();
        for disk in &sys_disks {
            if disk.total_space() != 0 {
                disks.push(Disk {
                    fs: disk.file_system().to_str().unwrap().to_string(),
                    used_space: disk.total_space() - disk.available_space(),
                    total_space: disk.total_space(),
                    used_space_readable: format_size(
                        disk.total_space() - disk.available_space(),
                        DECIMAL,
                    ),
                    total_space_readable: format_size(disk.total_space(), DECIMAL),
                });
            }
        }

        return Ok(Template::render(
            "sysinfo",
            context! {
                title: "Server information",
                theme: get_theme(jar),
                is_logged_in: is_logged_in(&jar),
                hires: get_bool_cookie(jar, "hires"),
                admin: get_session(&jar).1 == 0,
                smallhead: get_bool_cookie(jar, "smallhead"),
                username: username,
                total_mem: total_mem,
                total_mem_readable: format_size(total_mem, DECIMAL),
                used_mem: used_mem,
                used_mem_readable: format_size(used_mem, DECIMAL),
                sys_name: sys_name,
                sys_ver: sys_ver,
                hostname: hostname,
                disks: disks
            },
        ));
    } else {
        return Err(Status::Forbidden);
    }
}

#[get("/upload")]
fn uploader(jar: &CookieJar<'_>) -> Result<Template, Status> {
    if is_logged_in(&jar) {
        let (username, perms) = get_session(jar);

        if perms != 0 {
            return Err(Status::Forbidden);
        }

        return Ok(Template::render(
            "upload",
            context! {
                title: "File uploader",
                theme: get_theme(jar),
                is_logged_in: is_logged_in(&jar),
                hires: get_bool_cookie(jar, "hires"),
                smallhead: get_bool_cookie(jar, "smallhead"),
                username: username,
                admin: perms == 0,
                filebrowser: !get_bool_cookie(jar, "filebrowser"),
                uploadedfiles: vec![MirrorFile { name: "".to_string(), ext: "".to_string(), icon: "default".to_string(), size: String::new()}]
            },
        ));
    } else {
        return Err(Status::Forbidden);
    }
}

#[get("/login")]
fn login_page(jar: &CookieJar<'_>) -> Result<Template, Redirect> {
    if is_logged_in(&jar) {
        let perms = get_session(jar).1;
        if perms == 0 {
            return Err(Redirect::to("/upload"));
        } else {
            return Err(Redirect::to("/"));
        }
    }

    Ok(Template::render(
        "login",
        context! {
            title: "Login",
            theme: get_theme(jar),
            is_logged_in: is_logged_in(&jar),
            username: "",
            admin: false,
            hires: get_bool_cookie(jar, "hires"),
            smallhead: get_bool_cookie(jar, "smallhead"),
            message: ""
        },
    ))
}

#[post("/login?<next>", data = "<user>")]
async fn login(
    db: Connection<Db>,
    user: Form<MarmakUser>,
    jar: &CookieJar<'_>,
    ip: XForwardedFor<'_>,
    next: Option<&str>,
) -> Result<Redirect, Template> {
    if let Some(db_user) = login_user(db, &user.username, &user.password, &ip.0, true).await {
        if !get_bool_cookie(&jar, "nooverride") {
            if let Some(mirror_settings) = db_user.mirror_settings {
                let decoded: HashMap<String, String> =
                    serde_json::from_str(&mirror_settings).expect("Failed to parse JSON");

                for (key, value) in decoded {
                    let mut now = OffsetDateTime::now_utc();
                    now += Duration::days(365);
                    let mut cookie = Cookie::new(key, value);
                    cookie.set_expires(now);
                    jar.add(cookie);
                }
            }
        }

        jar.add_private(Cookie::new(
            "session",
            format!(
                "{}.{}",
                &user.username,
                &db_user.perms.unwrap_or_default().to_string()
            ),
        ));

        println!("Login for user {} from {} succeeded", &user.username, &ip.0);

        let mut redirect_url = next.unwrap_or("/").to_string();

        if redirect_url == "/upload" {
            return Ok(Redirect::to("/"));
        }

        if db_user.perms.unwrap_or(1) == 0 {
            redirect_url = next.unwrap_or("/upload").to_string();
        }

        return Ok(Redirect::to(redirect_url));
    } else {
        println!(
            "Failed login attempt to user {} with password {} from {}",
            &user.username, &user.password, &ip.0
        );
        return Err(Template::render(
            "login",
            context! {
                title: "Login",
                theme: get_theme(jar),
                is_logged_in: is_logged_in(&jar),
                admin: get_session(&jar).1 == 0,
                hires: get_bool_cookie(jar, "hires"),
                smallhead: get_bool_cookie(jar, "smallhead"),
                message: "Incorrect username or password"
            },
        ));
    }
}

#[get("/direct?<token>&<to>")]
async fn direct<'a>(
    db: Connection<Db>,
    jar: &CookieJar<'_>,
    token: Option<String>,
    to: Option<String>,
    ip: XForwardedFor<'_>,
) -> Result<Redirect, Status> {
    if let Some(token) = token {
        if is_logged_in(&jar) {
            let perms = get_session(jar).1;
            return Ok(Redirect::to(if perms == 1 { "/upload" } else { "/" }));
        }

        let private_key = fs::read("private.key").map_err(|_| Status::InternalServerError)?;
        let rsa =
            Rsa::private_key_from_pem(&private_key).map_err(|_| Status::InternalServerError)?;

        let encrypted_data = base64::engine::general_purpose::URL_SAFE
            .decode(&token.replace(".", "="))
            .map_err(|_| Status::BadRequest)?;

        let mut decrypted_data = vec![0; rsa.size() as usize];
        let decrypted_len = rsa
            .private_decrypt(&encrypted_data, &mut decrypted_data, Padding::PKCS1)
            .map_err(|_| Status::InternalServerError)?;

        let decrypted_data = &decrypted_data[..decrypted_len];

        let mut json_bytes = Vec::new();
        BASE64_STANDARD
            .decode_vec(decrypted_data, &mut json_bytes)
            .map_err(|_| Status::BadRequest)?;

        let json = String::from_utf8(json_bytes).map_err(|_| Status::InternalServerError)?;
        let received_user: UserToken =
            serde_json::from_str(&json).map_err(|_| Status::BadRequest)?;

        if let Some(db_user) = login_user(db, &received_user.username, "", ip.0, false).await {
            if !get_bool_cookie(&jar, "nooverride") {
                if let Some(mirror_settings) = db_user.mirror_settings {
                    let decoded: HashMap<String, String> =
                        serde_json::from_str(&mirror_settings).expect("Failed to parse JSON");

                    for (key, value) in decoded {
                        let mut now = OffsetDateTime::now_utc();
                        now += Duration::days(365);
                        let mut cookie = Cookie::new(key, value);
                        cookie.set_expires(now);
                        jar.add(cookie);
                    }
                }
            }

            jar.add_private(Cookie::new(
                "session",
                format!(
                    "{}.{}",
                    received_user.username,
                    db_user.perms.unwrap_or_default()
                ),
            ));
            return Ok(Redirect::to("/upload"));
        }

        return Ok(Redirect::to("/login"));
    }

    if let Some(_to) = to {
        if is_logged_in(&jar) {
            if let Some(db_user) = fetch_user(db, get_session(jar).0.as_str()).await {
                let user_data =
                    json!({"username": get_session(jar).0, "password_hash": db_user.password});
                let b64token = BASE64_STANDARD.encode(user_data.to_string());

                let public_key_pem =
                    fs::read_to_string("public.key").expect("Failed to read public key");
                let rsa = Rsa::public_key_from_pem(public_key_pem.as_bytes())
                    .expect("Invalid public key");

                let mut encrypted_data = vec![0; rsa.size() as usize];
                rsa.public_encrypt(b64token.as_bytes(), &mut encrypted_data, Padding::PKCS1)
                    .expect("Encryption failed");

                let encrypted_b64 =
                    base64::engine::general_purpose::URL_SAFE.encode(encrypted_data);
                let redirect_url = format!(
                    "https://account.marmak.net.pl/direct?token={}",
                    encrypted_b64
                );

                return Ok(Redirect::to(redirect_url));
            } else {
                return Err(Status::Forbidden);
            }
        } else {
            return Err(Status::Forbidden);
        }
    }

    Err(Status::BadRequest)
}

#[get("/settings/fetch")]
async fn fetch_settings(
    db: Connection<Db>,
    jar: &CookieJar<'_>,
) -> Result<RawHtml<&'static str>, Status> {
    if is_logged_in(&jar) {
        let username = get_session(jar).0;

        if let Some(db_user) = fetch_user(db, username.as_str()).await {
            let decoded: HashMap<String, String> =
                serde_json::from_str(&db_user.mirror_settings.unwrap())
                    .expect("Failed to parse JSON");

            for (key, value) in decoded {
                let mut now = OffsetDateTime::now_utc();
                now += Duration::days(365);
                let mut cookie = Cookie::new(key, value);
                cookie.set_expires(now);
                jar.add(cookie);
            }
        }

        return Ok(RawHtml("<script>alert(\"Settings fetched successfully!\");window.location.replace(\"/settings\");</script>"));
    } else {
        return Err(Status::Forbidden);
    }
}

#[get("/settings/sync")]
async fn sync_settings(
    db: Connection<Db>,
    jar: &CookieJar<'_>,
) -> Result<RawHtml<&'static str>, Status> {
    if is_logged_in(&jar) {
        let username = get_session(jar).0;

        let keys = vec![
            "lang",
            "useajax",
            "hires",
            "smallhead",
            "theme",
            "nolang",
            "nooverride",
        ];

        let mut cookie_map: HashMap<String, Option<String>> = HashMap::new();
        for key in keys {
            let value = jar.get(key).map(|cookie| cookie.value().to_string());
            cookie_map.insert(key.to_string(), value);
        }

        let settings = serde_json::to_string(&cookie_map).expect("Failed to serialize cookie data");

        db::update_settings(db, username.as_str(), settings.as_str()).await;

        return Ok(RawHtml("<script>alert(\"Settings saved successfully!\");window.location.replace(\"/settings\");</script>"));
    } else {
        return Err(Status::Forbidden);
    }
}

#[get("/logout")]
fn logout(jar: &CookieJar<'_>) -> Redirect {
    jar.remove_private("session");
    Redirect::to("/login")
}

#[catch(404)]
fn not_found(req: &Request) -> Template {
    let jar = req.cookies();

    Template::render(
        "error/404",
        context! {
            title: "Error 404",
            theme: get_theme(jar),
            is_logged_in: is_logged_in(&jar),
            admin: get_session(&jar).1 == 0,
            hires: get_bool_cookie(jar, "hires"),
            smallhead: get_bool_cookie(jar, "smallhead"),
        },
    )
}

#[catch(400)]
fn bad_request(req: &Request) -> Template {
    let jar = req.cookies();

    Template::render(
        "error/400",
        context! {
            title: "Error 400",
            theme: get_theme(jar),
            is_logged_in: is_logged_in(&jar),
            admin: get_session(&jar).1 == 0,
            hires: get_bool_cookie(jar, "hires"),
            smallhead: get_bool_cookie(jar, "smallhead"),
        },
    )
}

#[catch(500)]
fn internal_server_error(req: &Request) -> Template {
    let jar = req.cookies();

    Template::render(
        "error/500",
        context! {
            title: "Error 500",
            theme: get_theme(jar),
            is_logged_in: is_logged_in(&jar),
            admin: get_session(&jar).1 == 0,
            hires: get_bool_cookie(jar, "hires"),
            smallhead: get_bool_cookie(jar, "smallhead"),
        },
    )
}

#[catch(422)]
fn unprocessable_entry(req: &Request) -> Template {
    let jar = req.cookies();

    Template::render(
        "error/404",
        context! {
            title: "Error 404",
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
    Redirect::to(format!("/login?next={}", req.uri()))
}

#[launch]
fn rocket() -> _ {
    let config = Config::load();

    let rocket = rocket::build()
        .manage(config)
        .attach(Template::fairing())
        .attach(api::build_api())
        .attach(Db::init())
        .register("/", catchers![
            not_found,
            unprocessable_entry,
            forbidden,
            internal_server_error,
            bad_request
        ])
        .mount("/", routes![
            settings,
            download,
            index,
            upload,
            uploader,
            login,
            login_page,
            logout,
            sysinfo,
            fetch_settings,
            sync_settings,
            direct
        ]);

    rocket
}


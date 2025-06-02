use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use ::sysinfo::{Disks, RefreshKind, System};
use humansize::{format_size, DECIMAL};
use rocket::{
    data::ToByteUnit,
    fairing::AdHoc,
    http::{ContentType, CookieJar, Status},
    serde::json::Json,
    Data, Request,
};
use rocket_multipart_form_data::{
    MultipartFormData, MultipartFormDataField, MultipartFormDataOptions, Repetition,
};

use crate::{
    read_dirs, read_files,
    utils::{get_extension_from_filename, get_session, is_logged_in, is_restricted},
    Config, Disk, Host, MirrorFile, Sysinfo,
};

#[derive(serde::Serialize)]
struct MirrorInfo {
    version: String,
}

#[derive(serde::Serialize)]
struct Error {
    message: String,
}

#[derive(serde::Serialize)]
struct User {
    username: String,
    scope: String,
    perms: i32,
    settings: HashMap<String, String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct UploadFile {
    name: String,
    url: Option<String>,
    error: Option<String>,
    icon: Option<String>,
}

#[get("/listing/<file..>")]
async fn listing(
    file: PathBuf,
    jar: &CookieJar<'_>,
    config: &rocket::State<Config>,
) -> Result<Json<Vec<MirrorFile>>, Status> {
    let path = Path::new("/").join(file.clone()).display().to_string();

    let mut file_list = read_files(&path).unwrap_or_default();
    let mut dir_list = read_dirs(&path).unwrap_or_default();

    if dir_list.is_empty() && file_list.is_empty() {
        return Err(Status::NotFound);
    }

    if is_restricted(Path::new("files/").join(file.clone()), &jar) {
        return Err(Status::Forbidden);
    }

    dir_list.retain(|x| !config.hidden_files.contains(&x.name));
    file_list.retain(|x| !config.hidden_files.contains(&x.name));

    dir_list.sort();
    file_list.sort();

    dir_list.append(&mut file_list);

    Ok(Json(dir_list))
}

#[get("/sysinfo")]
fn sysinfo(jar: &CookieJar<'_>) -> Result<Json<Sysinfo>, Status> {
    if is_logged_in(&jar) {
        let mut sys = System::new_all();

        sys.refresh_specifics(RefreshKind::without_processes(RefreshKind::without_cpu(
            RefreshKind::everything(),
        )));

        let total_mem = sys.total_memory();
        let used_mem = sys.used_memory();
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

        return Ok(Json(Sysinfo {
            total_mem: total_mem,
            total_mem_readable: format_size(total_mem, DECIMAL),
            used_mem: used_mem,
            used_mem_readable: format_size(used_mem, DECIMAL),
            disks: disks,
        }));
    } else {
        return Err(Status::Forbidden);
    }
}

#[get("/user")]
fn user(jar: &CookieJar<'_>) -> Result<Json<User>, Status> {
    if is_logged_in(&jar) {
        let (username, perms) = get_session(jar);

        let keys = vec![
            "lang",
            "useajax",
            "hires",
            "smallhead",
            "theme",
            "nolang",
            "nooverride",
        ];

        let mut settings: HashMap<String, String> = HashMap::new();
        for key in keys {
            let value = jar.get(key).map(|cookie| cookie.value().to_string());
            settings.insert(key.to_string(), value.unwrap_or_default());
        }

        Ok(Json(User {
            username,
            scope: match perms {
                0 => "admin".to_string(),
                _ => "user".to_string(),
            },
            perms,
            settings,
        }))
    } else {
        return Err(Status::Forbidden);
    }
}

#[post("/upload", data = "<data>")]
async fn upload(
    content_type: &ContentType,
    data: Data<'_>,
    jar: &CookieJar<'_>,
    host: Host<'_>,
) -> Result<Json<Vec<UploadFile>>, Status> {
    if is_logged_in(&jar) {
        let perms = get_session(jar).1;

        if perms != 0 {
            return Err(Status::Forbidden);
        }

        let options = MultipartFormDataOptions::with_multipart_form_data_fields(vec![
            MultipartFormDataField::file("files")
                .repetition(Repetition::infinite())
                .size_limit(u64::from(1.gigabytes())),
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

        let mut uploaded_files: Vec<UploadFile> = Vec::new();

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
                                uploaded_files.push(UploadFile {
                                    name: file_name.to_string(),
                                    url: Some(format!(
                                        "http://{}/{}/{}",
                                        host.0, user_path, file_name
                                    )),
                                    icon: Some(icon),
                                    error: None,
                                });
                            } else {
                                eprintln!("Failed to open temp file for: {}", file_name);
                                return Err(Status::InternalServerError);
                            }
                        }
                        Err(err) => {
                            uploaded_files.push(UploadFile {
                                name: file_name.to_string(),
                                url: None,
                                icon: None,
                                error: Some(format!(
                                    "Failed to create target file {}: {:?}",
                                    upload_path, err
                                )),
                            });
                            eprintln!("Failed to create target file {}: {:?}", upload_path, err);
                            continue;
                        }
                    }
                } else {
                    eprintln!("A file was uploaded without a name, skipping.");
                    continue;
                }
            }

            return Ok(Json(uploaded_files));
        } else {
            return Err(Status::BadRequest);
        }
    } else {
        return Err(Status::Forbidden);
    }
}

#[get("/")]
async fn index() -> Json<MirrorInfo> {
    Json(MirrorInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[catch(default)]
fn default(status: Status, _req: &Request) -> Json<Error> {
    Json(Error {
        message: format!("{}", status),
    })
}

pub fn build_api() -> AdHoc {
    AdHoc::on_ignite("API", |rocket| async {
        rocket
            .mount("/api", routes![index, listing, sysinfo, user, upload])
            .register("/api", catchers![default])
    })
}

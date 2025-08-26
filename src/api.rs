use std::{
    collections::HashMap,
    fs::{self, remove_dir, remove_file},
    io::{Cursor, Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
};

use ::sysinfo::{Disks, RefreshKind, System};
use rocket::{
    data::ToByteUnit,
    fairing::AdHoc,
    http::{ContentType, CookieJar, Status},
    serde::json::Json,
    Data, Request, State,
};
use rocket_db_pools::Connection;
use rocket_multipart_form_data::{
    MultipartFormData, MultipartFormDataField, MultipartFormDataOptions, Repetition,
};
use zip::write::SimpleFileOptions;

use crate::{
    db::{get_downloads, FileDb},
    read_files,
    utils::{
        add_path_to_zip, format_size, get_extension_from_filename, get_real_path,
        get_real_path_with_perms, get_session, is_logged_in, is_restricted, map_io_error_to_status,
        read_dirs_async,
    },
    Cached, Config, Disk, FileSizes, Host, MirrorFile, Sysinfo,
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

#[derive(serde::Deserialize)]
struct FileList(Vec<String>);

#[derive(serde::Deserialize)]
struct RenameRequest {
    name: String,
}

#[get("/listing/<file..>")]
async fn listing(
    file: PathBuf,
    jar: &CookieJar<'_>,
    config: &rocket::State<Config>,
    sizes: &State<FileSizes>,
) -> Result<Cached<Json<Vec<MirrorFile>>>, Status> {
    let username = get_session(jar).0;

    let path = get_real_path(&file, username)?.0.display().to_string();

    let mut file_list = read_files(&path).map_err(map_io_error_to_status)?;
    let mut dir_list = read_dirs_async(&path, sizes)
        .await
        .map_err(map_io_error_to_status)?;

    if is_restricted(&Path::new("files/").join(&file), jar) {
        return Err(Status::Forbidden);
    }

    dir_list.retain(|x| !config.hidden_files.contains(&x.name));
    file_list.retain(|x| !config.hidden_files.contains(&x.name));

    dir_list.sort();
    file_list.sort();

    dir_list.append(&mut file_list);

    Ok(Cached {
        response: Json(dir_list),
        header: "no-cache",
    })
}

#[get("/<file..>", rank = 1)]
async fn file_with_downloads(
    db: Connection<FileDb>,
    file: PathBuf,
) -> Result<Cached<Json<MirrorFile>>, Status> {
    let path = Path::new("files/").join(&file);
    let file = file.display().to_string();

    if !&path.exists() {
        return Err(Status::NotFound);
    }

    let md = fs::metadata(&path).map_err(|_| Status::InternalServerError)?;

    let name = path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap_or_default()
        .to_string();
    let downloads = get_downloads(db, &file).await.unwrap_or(0);
    let mut icon = path.extension().unwrap().to_str().unwrap_or("default");

    if !Path::new(&format!("files/static/images/icons/{}.png", &icon)).exists() {
        icon = "default";
    }

    Ok(Cached {
        response: Json(MirrorFile {
            name,
            ext: path
                .extension()
                .unwrap()
                .to_str()
                .unwrap_or_default()
                .to_string(),
            icon: icon.to_string(),
            size: md.len(),
            downloads: Some(downloads),
        }),
        header: "no-cache",
    })
}

#[get("/<file..>", rank = 1)]
async fn file(file: PathBuf) -> Result<Cached<Json<MirrorFile>>, Status> {
    let path = Path::new("files/").join(&file);

    if !&path.exists() {
        return Err(Status::NotFound);
    }

    let md = fs::metadata(&path).map_err(|_| Status::InternalServerError)?;

    let name = path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap_or_default()
        .to_string();
    let mut icon = path
        .extension()
        .unwrap_or_default()
        .to_str()
        .unwrap_or("default");

    if !Path::new(&format!("files/static/images/icons/{}.png", &icon)).exists() {
        icon = "default";
    }

    Ok(Cached {
        response: Json(MirrorFile {
            name,
            ext: path
                .extension()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default()
                .to_string(),
            icon: icon.to_string(),
            size: md.len(),
            downloads: None,
        }),
        header: "no-cache",
    })
}

#[patch("/<file..>", data = "<rename_req>")]
async fn rename(
    file: PathBuf,
    jar: &CookieJar<'_>,
    rename_req: Json<RenameRequest>,
) -> Result<Json<MirrorFile>, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
        let (username, perms) = get_session(jar);

        let path = get_real_path_with_perms(&file, username, perms)?.0;

        if !path.exists() {
            return Err(Status::NotFound);
        }

        let parent = path.parent().ok_or(Status::InternalServerError)?;
        let new_path = parent.join(&rename_req.name);

        fs::rename(&path, &new_path).map_err(map_io_error_to_status)?;

        let md = fs::metadata(&new_path).map_err(map_io_error_to_status)?;
        let name = new_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap_or_default()
            .to_string();

        let mut icon = new_path
            .extension()
            .unwrap_or_default()
            .to_str()
            .unwrap_or("default");

        if !Path::new(&format!("files/static/images/icons/{}.png", &icon)).exists() {
            icon = "default";
        }

        Ok(Json(MirrorFile {
            name,
            ext: new_path
                .extension()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default()
                .to_string(),
            icon: icon.to_string(),
            size: md.len(),
            downloads: None,
        }))
    }
}

#[delete("/<file..>")]
async fn delete<'a>(file: PathBuf, jar: &CookieJar<'_>) -> Result<Status, (Status, Json<Error>)> {
    if !is_logged_in(jar) {
        return Ok(Status::Unauthorized);
    } else {
        let (username, perms) = get_session(jar);

        let path = get_real_path_with_perms(&file, username, perms)
            .map_err(|e| {
                (
                    e,
                    Json(Error {
                        message: format!("{} {}", e.code, e.reason_lossy()),
                    }),
                )
            })?
            .0;

        if !path.exists() {
            return Ok(Status::NotFound);
        }

        if path.is_dir() {
            if path
                .read_dir()
                .map(|mut i| i.next().is_none())
                .unwrap_or(false)
            {
                return match remove_dir(path) {
                    Ok(_) => Ok(Status::NoContent),
                    Err(e) => Err((
                        Status::InternalServerError,
                        Json(Error {
                            message: format!("An error occured: {}", e),
                        }),
                    )),
                };
            } else {
                return Err((
                    Status::Conflict,
                    Json(Error {
                        message: "Directory is not empty!".to_string(),
                    }),
                ));
            }
        }

        return match remove_file(path) {
            Ok(_) => Ok(Status::NoContent),
            Err(e) => Err((
                Status::InternalServerError,
                Json(Error {
                    message: e.to_string(),
                }),
            )),
        };
    }
}

#[get("/sysinfo")]
fn sysinfo(jar: &CookieJar<'_>) -> Result<Cached<Json<Sysinfo>>, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
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
                    used_space_readable: format_size(disk.total_space() - disk.available_space()),
                    total_space_readable: format_size(disk.total_space()),
                });
            }
        }

        Ok(Cached {
            response: Json(Sysinfo {
                total_mem: total_mem,
                total_mem_readable: format_size(total_mem),
                used_mem: used_mem,
                used_mem_readable: format_size(used_mem),
                disks: disks,
            }),
            header: "no-cache",
        })
    }
}

#[get("/user")]
fn user(jar: &CookieJar<'_>) -> Result<Cached<Json<User>>, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
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

        Ok(Cached {
            response: Json(User {
                username,
                scope: match perms {
                    0 => "admin".to_string(),
                    _ => "user".to_string(),
                },
                perms,
                settings,
            }),
            header: "no-cache",
        })
    }
}

#[post("/upload?<path>", data = "<data>")]
async fn upload(
    content_type: &ContentType,
    data: Data<'_>,
    jar: &CookieJar<'_>,
    host: Host<'_>,
    path: Option<String>,
) -> Result<Json<Vec<UploadFile>>, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
        let (username, perms) = get_session(jar);

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

        if let Some(query_path) = path {
            user_path = query_path.trim_matches('/').to_string();
        }

        let is_private = user_path.starts_with("private");
        if !is_private && perms != 0 {
            return Err(Status::Forbidden);
        }

        print!("is_private: {}", is_private);

        let base_path = if is_private {
            format!(
                "files/private/{}/{}",
                username,
                user_path.trim_start_matches("private")
            )
        } else {
            format!("files/{}", user_path)
        };

        print!("base_path: {}", base_path);

        let mut uploaded_files: Vec<UploadFile> = Vec::new();

        if let Some(file_fields) = form_data.files.get("files") {
            for file_field in file_fields {
                if let Some(file_name) = &file_field.file_name {
                    let normalized_path = file_name.replace('\\', "/");
                    let file_name = &Path::new(&normalized_path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap()
                        .to_string();

                    let upload_path = format!("{}/{}", base_path, file_name);

                    let mut file =
                        std::fs::File::create(&upload_path).map_err(map_io_error_to_status)?;
                    let mut temp_file =
                        std::fs::File::open(&file_field.path).map_err(map_io_error_to_status)?;

                    let mut buffer = Vec::new();
                    let _ = temp_file.read_to_end(&mut buffer);

                    let _ = file.write_all(&buffer);

                    let ext = get_extension_from_filename(file_name)
                        .unwrap_or_else(|| "")
                        .to_lowercase();
                    let mut icon = ext.as_str();
                    if !Path::new(&format!("files/static/images/icons/{}.png", &icon)).exists() {
                        icon = "default";
                    }

                    uploaded_files.push(UploadFile {
                        name: file_name.to_string(),
                        url: Some(format!("http://{}/{}/{}", host.0, user_path, file_name)),
                        icon: Some(icon.to_string()),
                        error: None,
                    });
                } else {
                    eprintln!("A file was uploaded without a name, skipping.");
                    continue;
                }
            }

            return Ok(Json(uploaded_files));
        } else {
            return Err(Status::BadRequest);
        }
    }
}

#[post("/zip", data = "<data>")]
async fn download_zip(
    content_type: &ContentType,
    data: Data<'_>,
    jar: &CookieJar<'_>,
) -> Result<Option<(ContentType, Vec<u8>)>, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
        let mut options = MultipartFormDataOptions::new();
        options
            .allowed_fields
            .push(MultipartFormDataField::raw("files"));

        let multipart_form_data = MultipartFormData::parse(content_type, data, options)
            .await
            .ok()
            .unwrap();
        let files_field = multipart_form_data.raw.get("files").unwrap();

        let file_json = String::from_utf8(files_field[0].raw.clone()).ok().unwrap();
        let file_list: FileList = serde_json::from_str(&file_json).ok().unwrap();

        let mut zip_buf = Cursor::new(Vec::new());
        let mut zip_writer = zip::ZipWriter::new(&mut zip_buf);
        let zip_options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        let root_base = PathBuf::from("files");
        for path_encoded in file_list.0 {
            let path_decoded = urlencoding::decode(&path_encoded).ok().unwrap();
            let full_path = format!("files{}", path_decoded.deref());
            let fs_path = PathBuf::from(&full_path);
            if fs_path.exists() {
                if let Err(e) = add_path_to_zip(&mut zip_writer, &root_base, &fs_path, zip_options)
                {
                    eprintln!("Failed to add {:?} to zip: {}", fs_path, e);
                }
            }
        }

        zip_writer.finish().ok().unwrap();
        let zip_bytes = zip_buf.into_inner();

        Ok(Some((ContentType::new("application", "zip"), zip_bytes)))
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
    AdHoc::on_ignite("API", |mut rocket| async {
        let config = Config::load();
        rocket = rocket
            .mount(
                "/api",
                routes![index, listing, sysinfo, user, upload, delete, rename,],
            )
            .register("/api", catchers![default]);

        if config.enable_file_db {
            rocket = rocket.mount("/api", routes![file_with_downloads])
        } else {
            rocket = rocket.mount("/api", routes![file])
        }

        if config.enable_zip_downloads {
            rocket = rocket.mount("/api", routes![download_zip])
        }

        rocket
    })
}

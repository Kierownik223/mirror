use std::{
    fs::{self, remove_dir, remove_file},
    io::{Cursor, Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
};

use ::sysinfo::{Disks, RefreshKind, System};
use audiotags::Tag;
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
    config::CONFIG,
    db::{get_downloads, FileDb},
    jwt::JWT,
    read_files,
    responders::{ApiResponse, ApiResult},
    utils::{
        add_path_to_zip, format_size, get_extension_from_filename, get_extension_from_path,
        get_genre, get_name_from_path, get_real_path, get_real_path_with_perms, is_restricted,
        map_io_error_to_status, read_dirs_async,
    },
    Disk, FileSizes, Host, MirrorFile, Sysinfo,
};

#[derive(serde::Serialize)]
struct MirrorInfo {
    version: String,
}

#[derive(serde::Serialize)]
pub struct ApiInfoResponse {
    message: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct UploadFile {
    name: String,
    url: Option<String>,
    error: Option<String>,
    icon: Option<String>,
}

#[derive(serde::Serialize)]
pub struct MusicFile {
    title: String,
    album: Option<String>,
    artist: Option<String>,
    year: Option<i32>,
    genre: Option<String>,
    track: Option<u16>,
    cover: bool,
}

#[derive(serde::Deserialize)]
struct FileList(Vec<String>);

#[derive(serde::Deserialize)]
struct RenameRequest {
    name: String,
}

#[get("/listing/<file..>")]
async fn listing(file: PathBuf, sizes: &State<FileSizes>, token: Result<JWT, Status>) -> ApiResult {
    let username = match token.as_ref() {
        Ok(token) => &token.claims.sub,
        Err(_) => &"Nobody".into(),
    };

    let path = get_real_path(&file, username.to_string())?.0;

    if path.is_file() {
        return Err(Status::NotAcceptable);
    }

    let path = path.display().to_string();

    let mut file_list = read_files(&path).map_err(map_io_error_to_status)?;
    let mut dir_list = read_dirs_async(&path, sizes)
        .await
        .map_err(map_io_error_to_status)?;

    if CONFIG.enable_login {
        if is_restricted(&Path::new("files/").join(&file), token.is_ok()) {
            return Err(Status::Forbidden);
        }
    }

    dir_list.retain(|x| !CONFIG.hidden_files.contains(&x.name));
    file_list.retain(|x| !CONFIG.hidden_files.contains(&x.name));

    dir_list.sort();
    file_list.sort();

    dir_list.append(&mut file_list);

    Ok(ApiResponse::Files(Json(dir_list)))
}

#[get("/<file..>", rank = 1)]
async fn file_with_downloads(
    db: Connection<FileDb>,
    file: PathBuf,
    token: Result<JWT, Status>,
) -> ApiResult {
    let username = match token.as_ref() {
        Ok(token) => &token.claims.sub,
        Err(_) => &"Nobody".into(),
    };

    let path = get_real_path(&file, username.to_string())?.0;
    let file = file.display().to_string();

    if !&path.exists() {
        return Err(Status::NotFound);
    }

    let md = fs::metadata(&path).map_err(|_| Status::InternalServerError)?;

    if md.is_dir() {
        return Err(Status::NotAcceptable);
    }

    let name = get_name_from_path(&path);
    let downloads = get_downloads(db, &file).await.unwrap_or(0);
    let ext = get_extension_from_path(&path);
    let mut icon = get_extension_from_path(&path);

    if !Path::new(&format!("files/static/images/icons/{}.png", &icon)).exists() {
        icon = "default".into();
    }

    if ext == "mp3" || ext == "m4a" || ext == "m4b" || ext == "flac" {
        if let Ok(tag) = Tag::new().read_from_path(&path) {
            let title = tag
                .title()
                .map(|s| s.to_string())
                .unwrap_or(get_name_from_path(&path));

            let artist = tag.artist().map(|s| s.to_string());
            let album = tag.album_title().map(|s| s.to_string());
            let genre = tag.genre().map(|s| get_genre(s).unwrap_or(s.to_string()));
            let year = tag.year();
            let track = tag.track_number();

            let cover = tag.album_cover().is_some();

            return Ok(ApiResponse::MusicFile(Json(MusicFile {
                title,
                album,
                artist,
                year,
                genre,
                track,
                cover,
            })));
        }
    }

    Ok(ApiResponse::File(Json(MirrorFile {
        name,
        ext,
        icon: icon.to_string(),
        size: md.len(),
        downloads: Some(downloads),
    })))
}

#[get("/<file..>", rank = 1)]
async fn file(file: PathBuf, token: Result<JWT, Status>) -> ApiResult {
    let username = match token.as_ref() {
        Ok(token) => &token.claims.sub,
        Err(_) => &"Nobody".into(),
    };

    let path = get_real_path(&file, username.to_string())?.0;

    if !&path.exists() {
        return Err(Status::NotFound);
    }

    let md = fs::metadata(&path).map_err(|_| Status::InternalServerError)?;

    if md.is_dir() {
        return Err(Status::NotAcceptable);
    }

    let name = get_name_from_path(&path);
    let ext = get_extension_from_path(&path);
    let mut icon = get_extension_from_path(&path);

    if !Path::new(&format!("files/static/images/icons/{}.png", &icon)).exists() {
        icon = "default".into();
    }

    if ext == "mp3" || ext == "m4a" || ext == "m4b" || ext == "flac" {
        if let Ok(tag) = Tag::new().read_from_path(&path) {
            let title = tag
                .title()
                .map(|s| s.to_string())
                .unwrap_or(get_name_from_path(&path));

            let artist = tag.artist().map(|s| s.to_string());
            let album = tag.album_title().map(|s| s.to_string());
            let genre = tag.genre().map(|s| get_genre(s).unwrap_or(s.to_string()));
            let year = tag.year();
            let track = tag.track_number();

            let cover = tag.album_cover().is_some();

            return Ok(ApiResponse::MusicFile(Json(MusicFile {
                title,
                album,
                artist,
                year,
                genre,
                track,
                cover,
            })));
        }
    }

    Ok(ApiResponse::File(Json(MirrorFile {
        name,
        ext,
        icon: icon.to_string(),
        size: md.len(),
        downloads: None,
    })))
}

#[patch("/<file..>", data = "<rename_req>")]
async fn rename(
    file: PathBuf,
    rename_req: Json<RenameRequest>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let token = token?;

    let username = token.claims.sub;
    let perms = token.claims.perms;

    let path = get_real_path_with_perms(&file, username, perms)?.0;

    if !path.exists() {
        return Err(Status::NotFound);
    }

    let parent = path.parent().ok_or(Status::InternalServerError)?;
    let new_path = parent.join(&rename_req.name);

    fs::rename(&path, &new_path).map_err(map_io_error_to_status)?;

    let md = fs::metadata(&new_path).map_err(map_io_error_to_status)?;

    let ext = get_extension_from_path(&new_path);

    let mut icon = ext.as_str();

    if !Path::new(&format!("files/static/images/icons/{}.png", &icon)).exists() {
        icon = "default";
    }

    Ok(ApiResponse::File(Json(MirrorFile {
        name: get_extension_from_path(&new_path),
        ext: get_extension_from_path(&new_path),
        icon: icon.to_string(),
        size: md.len(),
        downloads: None,
    })))
}

#[delete("/<file..>")]
async fn delete<'a>(file: PathBuf, token: Result<JWT, Status>) -> ApiResult {
    let token = token?;

    let username = token.claims.sub;
    let perms = token.claims.perms;

    let path = get_real_path_with_perms(&file, username, perms)?.0;

    if !path.exists() {
        return Err(Status::NotFound);
    }

    if path.is_dir() {
        if path
            .read_dir()
            .map(|mut i| i.next().is_none())
            .unwrap_or(false)
        {
            return match remove_dir(path) {
                Ok(_) => Err(Status::NoContent),
                Err(e) => Ok(ApiResponse::MessageStatus((
                    Status::InternalServerError,
                    Json(ApiInfoResponse {
                        message: format!("An error occured: {}", e),
                    }),
                ))),
            };
        } else {
            return Ok(ApiResponse::MessageStatus((
                Status::Conflict,
                Json(ApiInfoResponse {
                    message: "Directory is not empty!".to_string(),
                }),
            )));
        }
    }

    return match remove_file(path) {
        Ok(_) => Err(Status::NoContent),
        Err(e) => Ok(ApiResponse::MessageStatus((
            Status::InternalServerError,
            Json(ApiInfoResponse {
                message: e.to_string(),
            }),
        ))),
    };
}

#[get("/sysinfo?<use_si>")]
fn sysinfo(token: Result<JWT, Status>, use_si: Option<&str>, jar: &CookieJar<'_>) -> ApiResult {
    let _token = token?;

    let mut sys = System::new_all();

    let use_si = match use_si {
        Some(u) => match u {
            "false" => false,
            _ => true,
        },
        None => crate::utils::get_bool_cookie(jar, "use_si", true),
    };

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
                fs: disk.file_system().to_str().unwrap_or("unknown").to_string(),
                used_space: disk.total_space() - disk.available_space(),
                total_space: disk.total_space(),
                used_space_readable: format_size(
                    disk.total_space() - disk.available_space(),
                    use_si,
                ),
                total_space_readable: format_size(disk.total_space(), use_si),
                mount_point: disk.mount_point().display().to_string(),
            });
        }
    }

    Ok(ApiResponse::Sysinfo(Json(Sysinfo {
        total_mem: total_mem,
        total_mem_readable: format_size(total_mem, use_si),
        used_mem: used_mem,
        used_mem_readable: format_size(used_mem, use_si),
        disks: disks,
    })))
}

#[post("/upload?<path>", data = "<data>")]
async fn upload(
    content_type: &ContentType,
    data: Data<'_>,
    host: Host<'_>,
    path: Option<String>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let token = token?;

    let username = token.claims.sub;
    let perms = token.claims.perms;

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

    let base_path = if is_private {
        format!(
            "files/private/{}/{}",
            username,
            user_path.trim_start_matches("private")
        )
    } else {
        format!("files/{}", user_path)
    };

    let mut uploaded_files: Vec<UploadFile> = Vec::new();

    if let Some(file_fields) = form_data.files.get("files") {
        for file_field in file_fields {
            if let Some(file_name) = &file_field.file_name {
                let normalized_path = file_name.replace('\\', "/");
                let file_name = &get_name_from_path(&Path::new(&normalized_path).to_path_buf());

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

        return Ok(ApiResponse::UploadFiles(Json(uploaded_files)));
    } else {
        return Err(Status::BadRequest);
    }
}

#[post("/zip", data = "<data>")]
async fn download_zip(
    content_type: &ContentType,
    data: Data<'_>,
    token: Result<JWT, Status>,
) -> Result<Option<(ContentType, Vec<u8>)>, Status> {
    let _token = token?;

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
            if let Err(e) = add_path_to_zip(&mut zip_writer, &root_base, &fs_path, zip_options) {
                eprintln!("Failed to add {:?} to zip: {}", fs_path, e);
            }
        }
    }

    zip_writer.finish().ok().unwrap();
    let zip_bytes = zip_buf.into_inner();

    Ok(Some((ContentType::new("application", "zip"), zip_bytes)))
}

#[get("/")]
async fn index() -> Json<MirrorInfo> {
    Json(MirrorInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[catch(default)]
fn default(status: Status, _req: &Request) -> ApiResponse {
    ApiResponse::Message(Json(ApiInfoResponse {
        message: format!("{}", status),
    }))
}

pub fn build_api() -> AdHoc {
    AdHoc::on_ignite("API", |mut rocket| async {
        rocket = rocket
            .mount(
                "/api",
                routes![index, listing, sysinfo, upload, delete, rename,],
            )
            .register("/api", catchers![default]);

        if CONFIG.enable_file_db {
            rocket = rocket.mount("/api", routes![file_with_downloads])
        } else {
            rocket = rocket.mount("/api", routes![file])
        }

        if CONFIG.enable_zip_downloads {
            rocket = rocket.mount("/api", routes![download_zip])
        }

        rocket
    })
}

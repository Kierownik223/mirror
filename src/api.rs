use std::{
    collections::HashMap,
    fs::{self, create_dir, remove_dir, remove_dir_all, remove_file},
    io::{Cursor, ErrorKind, Read, Write},
    ops::Deref,
    path::{Path, PathBuf},
};

use ::sysinfo::{Disks, RefreshKind, System};
use audiotags::Tag;
use rocket::{
    data::ToByteUnit,
    fairing::AdHoc,
    http::{uri::Segments, ContentType, Status},
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
    db::{delete_file, FileDb},
    jwt::JWT,
    read_files, refresh_file_sizes,
    responders::{ApiResponse, ApiResult},
    utils::{add_path_to_zip, map_io_error_to_status, read_dirs_async},
    Disk, FileSizes, Host, MirrorFile, MirrorFileInternal, Sysinfo,
};

#[derive(serde::Serialize)]
struct MirrorInfo {
    version: String,
}

#[derive(serde::Serialize)]
pub struct ApiInfoResponse {
    message: String,
}

#[derive(serde::Serialize)]
pub struct MirrorFileWrapper {
    file: MirrorFile,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct UploadFile {
    name: String,
    url: Option<String>,
    error: Option<String>,
    icon: Option<String>,
    size: Option<u64>,
}

#[derive(serde::Serialize)]
pub struct MusicFile {
    file: MirrorFile,
    title: String,
    album: Option<String>,
    artist: Option<String>,
    year: Option<i32>,
    genre: Option<String>,
    track: Option<u16>,
    cover: bool,
}

impl MusicFile {
    pub fn get_genre(genre: &str) -> Result<String, Status> {
        let toml_str = fs::read_to_string("genres.toml").map_err(map_io_error_to_status)?;
        let parsed: toml::Value =
            toml::from_str(&toml_str).map_err(|_| Status::InternalServerError)?;
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
}

#[derive(serde::Serialize)]
pub struct VideoFile {
    pub file: MirrorFile,
    pub title: String,
    pub description: Option<String>,
}

impl VideoFile {
    pub fn load(path: &str, file: Option<MirrorFile>) -> Self {
        let mdpath = format!("files/video/metadata{}.md", path.replace("video/", ""));
        let mdpath = Path::new(mdpath.as_str());

        let mut vidtitle = MirrorFile::get_name_from_path(&Path::new(path).to_path_buf());

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

        Self {
            file: file.unwrap_or_default(),
            title: vidtitle,
            description: details,
        }
    }
}

#[derive(serde::Serialize)]
pub struct UploadLimits {
    perms: i32,
    upload_limit: u64,
    private_folder_quota: u64,
    private_folder_usage: u64,
}

#[derive(serde::Deserialize)]
struct FileList(Vec<String>);

#[derive(serde::Deserialize)]
struct NameRequest {
    name: String,
}

#[derive(serde::Serialize, PartialOrd, serde::Deserialize)]
pub struct SearchFile {
    pub name: String,
    pub full_path: String,
    pub icon: String,
    pub size: u64,
}

impl Eq for SearchFile {}

impl PartialEq for SearchFile {
    fn eq(&self, other: &Self) -> bool {
        (&self.name) == (&other.name)
    }
}

impl Ord for SearchFile {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

#[get("/listing/<segments..>")]
async fn listing(
    segments: Segments<'_, rocket::http::uri::fmt::Path>,
    sizes: &State<FileSizes>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let username = match token.as_ref() {
        Ok(token) => &token.claims.sub,
        Err(_) => &"Nobody".into(),
    };

    let file = segments.to_path_buf(true).map_err(|_| Status::BadRequest)?;

    let path = MirrorFile::get_real_path(&file, username.to_string())?.0;

    if path.is_file() {
        return Err(Status::NotAcceptable);
    }

    let path = path.display().to_string();

    let mut file_list = read_files(&path).map_err(map_io_error_to_status)?;
    let mut dir_list = read_dirs_async(&path, sizes)
        .await
        .map_err(map_io_error_to_status)?;

    if CONFIG.enable_login {
        if MirrorFile::is_restricted(&Path::new("files/").join(&file), token.is_ok()) {
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

#[get("/search?<q>")]
async fn search(
    q: Option<&str>,
    sizes: &State<FileSizes>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let perms = match token.as_ref() {
        Ok(token) => Some(token.claims.perms),
        Err(_) => None,
    };

    if let Some(q) = q {
        if q.len() < 3 {
            return Ok(ApiResponse::MessageStatus((
                Status::BadRequest,
                Json(ApiInfoResponse {
                    message: "Search query must be 3 characters or longer!".into(),
                }),
            )));
        }

        let mut results: Vec<SearchFile> = sizes
            .read()
            .await
            .iter()
            .map(|x| SearchFile {
                name: MirrorFile::get_name_from_path(&Path::new(&x.file).to_path_buf()),
                full_path: MirrorFile::get_virtual_path(&x.file),
                icon: if Path::new(&x.file).is_dir() {
                    "folder".into()
                } else {
                    MirrorFile::get_icon(&MirrorFile::get_name_from_path(
                        &Path::new(&x.file).to_path_buf(),
                    ))
                },
                size: x.size,
            })
            .collect();

        results.retain(|x| !CONFIG.hidden_files.contains(&x.name));
        results.retain(|x| x.name.contains(q));
        results.retain(|x| !MirrorFile::is_hidden_path_str(&x.full_path, perms));
        results.retain(|x| !x.full_path.starts_with("/private/"));

        if results.len() == 0 {
            return Ok(ApiResponse::MessageStatus((
                Status::NotFound,
                Json(ApiInfoResponse {
                    message: "No results found!".into(),
                }),
            )));
        }

        Ok(ApiResponse::SearchResults(Json(results)))
    } else {
        return Ok(ApiResponse::MessageStatus((
            Status::BadRequest,
            Json(ApiInfoResponse {
                message: "Search query must not be empty!".into(),
            }),
        )));
    }
}

#[get("/<segments..>", rank = 1)]
async fn file_with_downloads(
    db: Connection<FileDb>,
    segments: Segments<'_, rocket::http::uri::fmt::Path>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let file = segments.to_path_buf(true).map_err(|_| Status::BadRequest)?;
    display_file(Some(db), file, token).await
}

#[get("/<segments..>", rank = 1)]
async fn file(
    segments: Segments<'_, rocket::http::uri::fmt::Path>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let file = segments.to_path_buf(true).map_err(|_| Status::BadRequest)?;
    display_file(None, file, token).await
}

async fn display_file(
    db: Option<Connection<FileDb>>,
    path: PathBuf,
    token: Result<JWT, Status>,
) -> ApiResult {
    let username = match token.as_ref() {
        Ok(token) => &token.claims.sub,
        Err(_) => &"Nobody".into(),
    };

    let file = path.display().to_string();
    let path = MirrorFile::get_real_path(&path, username.to_string())?.0;

    let mirror_file = if let Some(db) = db {
        MirrorFileInternal::load(db, &path)
            .await
            .ok_or(Status::NotFound)?
            .mirror_file
    } else {
        MirrorFile::load(&path).ok_or(Status::NotFound)?
    };

    if mirror_file.is_dir() {
        return Err(Status::NotAcceptable);
    }

    if mirror_file.ext == "mp3"
        || mirror_file.ext == "m4a"
        || mirror_file.ext == "m4b"
        || mirror_file.ext == "flac"
    {
        if let Ok(tag) = Tag::new().read_from_path(&path) {
            let title = tag
                .title()
                .map(|s| s.to_string())
                .unwrap_or(MirrorFile::get_name_from_path(&path));

            let artist = tag.artist().map(|s| s.replace("\x00", "/"));
            let album = tag.album_title().map(|s| s.to_string());
            let genre = tag
                .genre()
                .map(|s| MusicFile::get_genre(s).unwrap_or(s.to_string()));
            let year = tag.year();
            let track = tag.track_number();

            let cover = tag.album_cover().is_some();

            return Ok(ApiResponse::MusicFile(Json(MusicFile {
                file: mirror_file,
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

    if mirror_file.ext == "mp4" || mirror_file.ext == "mkv" || mirror_file.ext == "webm" {
        let videopath = Path::new("/").join(file.clone()).display().to_string();

        return Ok(ApiResponse::VideoFile(Json(VideoFile::load(
            &videopath,
            Some(mirror_file),
        ))));
    }

    Ok(ApiResponse::File(Json(MirrorFileWrapper {
        file: mirror_file,
    })))
}

#[patch("/<segments..>", data = "<rename_req>")]
async fn rename_db(
    db: Connection<FileDb>,
    segments: Segments<'_, rocket::http::uri::fmt::Path>,
    rename_req: Json<NameRequest>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let file = segments.to_path_buf(true).map_err(|_| Status::BadRequest)?;
    perform_rename(Some(db), file, rename_req, token).await
}

#[patch("/<segments..>", data = "<rename_req>")]
async fn rename(
    segments: Segments<'_, rocket::http::uri::fmt::Path>,
    rename_req: Json<NameRequest>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let file = segments.to_path_buf(true).map_err(|_| Status::BadRequest)?;
    perform_rename(None, file, rename_req, token).await
}

async fn perform_rename(
    db: Option<Connection<FileDb>>,
    file: PathBuf,
    rename_req: Json<NameRequest>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let token = token?;

    let path = MirrorFile::get_real_path_with_perms(&file, token.claims.sub, token.claims.perms)?.0;

    if !path.exists() {
        return Err(Status::NotFound);
    }

    if let Some(db) = db {
        delete_file(db, &path.display().to_string().replacen("files/", "", 1)).await;
    }

    let parent = path.parent().ok_or(Status::InternalServerError)?;
    let new_path = parent.join(&rename_req.name);

    fs::rename(&path, &new_path).map_err(map_io_error_to_status)?;

    let mirror_file = MirrorFile::load(&new_path).ok_or(Status::NotFound)?;

    Ok(ApiResponse::File(Json(MirrorFileWrapper {
        file: mirror_file,
    })))
}

#[delete("/<segments..>?<recurse>")]
async fn delete_db<'a>(
    db: Connection<FileDb>,
    segments: Segments<'_, rocket::http::uri::fmt::Path>,
    token: Result<JWT, Status>,
    sizes: &State<FileSizes>,
    recurse: Option<bool>,
) -> ApiResult {
    let file = segments.to_path_buf(true).map_err(|_| Status::BadRequest)?;
    perform_delete(Some(db), file, token, sizes, recurse).await
}

#[delete("/<segments..>?<recurse>")]
async fn delete<'a>(
    segments: Segments<'_, rocket::http::uri::fmt::Path>,
    token: Result<JWT, Status>,
    sizes: &State<FileSizes>,
    recurse: Option<bool>,
) -> ApiResult {
    let file = segments.to_path_buf(true).map_err(|_| Status::BadRequest)?;
    perform_delete(None, file, token, sizes, recurse).await
}

async fn perform_delete(
    db: Option<Connection<FileDb>>,
    file: PathBuf,
    token: Result<JWT, Status>,
    sizes: &State<FileSizes>,
    recurse: Option<bool>,
) -> ApiResult {
    let token = token?;

    let path = MirrorFile::get_real_path_with_perms(&file, token.claims.sub, token.claims.perms)?.0;

    if !path.exists() {
        return Err(Status::NotFound);
    }

    if let Some(recurse) = recurse {
        if recurse
            && path
                .display()
                .to_string()
                .starts_with(format!("files/private/").as_str())
        {
            return match remove_dir_all(path) {
                Ok(_) => {
                    {
                        let mut state_lock = sizes.write().await;
                        *state_lock = refresh_file_sizes().await;
                    }
                    Err(Status::NoContent)
                }
                Err(e) => Ok(ApiResponse::MessageStatus((
                    Status::InternalServerError,
                    Json(ApiInfoResponse {
                        message: e.to_string(),
                    }),
                ))),
            };
        }
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

    return match remove_file(&path) {
        Ok(_) => {
            {
                let mut state_lock = sizes.write().await;
                *state_lock = refresh_file_sizes().await;
            }
            if let Some(db) = db {
                delete_file(db, &path.display().to_string().replacen("files/", "", 1)).await;
            }
            Err(Status::NoContent)
        }
        Err(e) => Ok(ApiResponse::MessageStatus((
            Status::InternalServerError,
            Json(ApiInfoResponse {
                message: e.to_string(),
            }),
        ))),
    };
}

#[post("/<segments..>")]
async fn share(
    db: Connection<FileDb>,
    segments: Segments<'_, rocket::http::uri::fmt::Path>,
    token: Result<JWT, Status>,
) -> ApiResult {
    let token = token?;

    let file = segments.to_path_buf(true).map_err(|_| Status::BadRequest)?;

    let path = MirrorFile::get_real_path_with_perms(&file, token.claims.sub, token.claims.perms)?.0;

    if !path.exists() {
        return Err(Status::NotFound);
    }

    if let Some(mirror_file) = MirrorFileInternal::load_and_share(db, &path).await {
        if let Some(id) = mirror_file.id {
            Ok(ApiResponse::MessageStatus((
                Status::Created,
                Json(ApiInfoResponse { message: id }),
            )))
        } else {
            Err(Status::InternalServerError)
        }
    } else {
        Err(Status::InternalServerError)
    }
}

#[put("/<segments..>", data = "<name_req>")]
async fn create_folder<'a>(
    segments: Segments<'_, rocket::http::uri::fmt::Path>,
    token: Result<JWT, Status>,
    name_req: Option<Json<NameRequest>>,
) -> ApiResult {
    let token = token?;

    let file = segments.to_path_buf(true).map_err(|_| Status::BadRequest)?;

    let path = MirrorFile::get_real_path_with_perms(&file, token.claims.sub, token.claims.perms)?.0;

    if !path.exists() && !name_req.is_some() {
        return match create_dir(path) {
            Ok(_) => Err(Status::Created),
            Err(e) => Ok(ApiResponse::MessageStatus((
                match e.kind() {
                    ErrorKind::NotFound => Status::NotFound,
                    ErrorKind::PermissionDenied => Status::Forbidden,
                    ErrorKind::StorageFull => Status::InsufficientStorage,
                    _ => Status::InternalServerError,
                },
                Json(ApiInfoResponse {
                    message: e.to_string(),
                }),
            ))),
        };
    } else if let Some(name) = name_req {
        return match create_dir(path.join(&name.name)) {
            Ok(_) => Err(Status::Created),
            Err(e) => Ok(ApiResponse::MessageStatus((
                match e.kind() {
                    ErrorKind::NotFound => Status::NotFound,
                    ErrorKind::PermissionDenied => Status::Forbidden,
                    ErrorKind::StorageFull => Status::InsufficientStorage,
                    _ => Status::InternalServerError,
                },
                Json(ApiInfoResponse {
                    message: e.to_string(),
                }),
            ))),
        };
    } else {
        Err(Status::BadRequest)
    }
}

#[get("/sysinfo")]
fn sysinfo(token: Result<JWT, Status>) -> ApiResult {
    let token = token?;

    if token.claims.perms != 0 {
        return Err(Status::Forbidden);
    }

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
                fs: disk.file_system().to_str().unwrap_or("unknown").to_string(),
                used_space: disk.total_space() - disk.available_space(),
                total_space: disk.total_space(),
                mount_point: disk.mount_point().display().to_string(),
            });
        }
    }

    Ok(ApiResponse::Sysinfo(Json(Sysinfo {
        total_mem: total_mem,
        used_mem: used_mem,
        disks: disks,
    })))
}

#[post("/upload?<path>&<share>", data = "<data>")]
async fn upload(
    content_type: &ContentType,
    data: Data<'_>,
    host: Host<'_>,
    path: Option<&str>,
    share: Option<&str>,
    token: Result<JWT, Status>,
    sizes: &State<FileSizes>,
) -> ApiResult {
    perform_upload(None, path, share, content_type, data, host, token, sizes).await
}

#[post("/upload?<path>&<share>", data = "<data>")]
async fn upload_db(
    db: Connection<FileDb>,
    content_type: &ContentType,
    data: Data<'_>,
    host: Host<'_>,
    path: Option<&str>,
    share: Option<&str>,
    token: Result<JWT, Status>,
    sizes: &State<FileSizes>,
) -> ApiResult {
    perform_upload(
        Some(db),
        path,
        share,
        content_type,
        data,
        host,
        token,
        sizes,
    )
    .await
}

async fn perform_upload(
    db: Option<Connection<FileDb>>,
    path: Option<&str>,
    share: Option<&str>,
    content_type: &ContentType,
    data: Data<'_>,
    host: Host<'_>,
    token: Result<JWT, Status>,
    sizes: &State<FileSizes>,
) -> ApiResult {
    let token = token?;

    let max_size = CONFIG
        .max_upload_sizes
        .get(&token.claims.perms.to_string())
        .unwrap_or(&(104857600 as u64));

    let options = MultipartFormDataOptions::with_multipart_form_data_fields(vec![
        MultipartFormDataField::file("files")
            .repetition(Repetition::infinite())
            .size_limit(*max_size),
        MultipartFormDataField::text("path"),
    ]);

    let form_data = match MultipartFormData::parse(content_type, data, options).await {
        Ok(data) => data,
        Err(err) => {
            eprintln!("Failed to parse multipart form data: {:?}", err);
            return Err(Status::BadRequest);
        }
    };

    if !Path::new("files/").exists() && !Path::new("files/").exists() {
        let result = fs::create_dir("files/").map_err(map_io_error_to_status);
        if let Err(error) = result {
            return Err(error);
        }
    }

    let mut user_path = form_data
        .texts
        .get("path")
        .and_then(|paths| paths.first().map(|p| p.text.trim_matches('/').to_string()))
        .unwrap_or(String::new());

    if user_path.is_empty() {
        user_path = if token.claims.perms == 0 {
            "uploads"
        } else {
            "private"
        }
        .to_string();
    }

    if let Some(query_path) = path {
        user_path = query_path.trim_matches('/').to_string();
    }

    user_path = urlencoding::decode(&user_path)
        .unwrap_or((&user_path).into())
        .to_string();

    let is_private = user_path.starts_with("private");
    if !is_private && token.claims.perms != 0 {
        return Err(Status::Forbidden);
    }

    let base_path = if is_private {
        format!(
            "files/private/{}/{}",
            &token.claims.sub,
            user_path.trim_start_matches("private")
        )
    } else {
        format!("files/{}", user_path)
    };

    if !Path::new(&base_path).exists() {
        let result = fs::create_dir_all(&base_path).map_err(map_io_error_to_status);
        if let Err(error) = result {
            return Err(error);
        }
    }

    let folder_quota = *(CONFIG
        .private_folder_quotas
        .get(&token.claims.perms.to_string())
        .unwrap_or(&1_u64));

    let folder_usage = sizes
        .read()
        .await
        .iter()
        .find(|entry| {
            entry.file.strip_suffix("/").unwrap_or_default().to_string()
                == format!("files/private/{}", &token.claims.sub)
        })
        .map(|entry| entry.size)
        .unwrap_or(0);

    if folder_quota != 0 && folder_usage >= folder_quota {
        return Err(Status::InsufficientStorage);
    }

    let mut uploaded_files: Vec<UploadFile> = Vec::new();

    if let Some(file_fields) = form_data.files.get("files") {
        for file_field in file_fields {
            if let Some(file_name) = &file_field.file_name {
                let normalized_path = file_name.replace('\\', "/");
                let file_name =
                    &MirrorFile::get_name_from_path(&Path::new(&normalized_path).to_path_buf());

                let upload_path = format!("{}/{}", base_path, file_name);

                let mut file =
                    std::fs::File::create(&upload_path).map_err(map_io_error_to_status)?;
                let mut temp_file =
                    std::fs::File::open(&file_field.path).map_err(map_io_error_to_status)?;

                let size = temp_file.metadata().map_err(map_io_error_to_status)?.len();

                if folder_quota != 0 && folder_usage + size >= folder_quota {
                    return Err(Status::InsufficientStorage);
                }

                let mut buffer = Vec::new();
                let _ = temp_file.read_to_end(&mut buffer);

                let _ = file.write_all(&buffer);
                {
                    let mut state_lock = sizes.write().await;
                    *state_lock = refresh_file_sizes().await;
                }

                uploaded_files.push(UploadFile {
                    name: file_name.to_string(),
                    url: Some(format!("http://{}/{}/{}", host.0, user_path, file_name)),
                    icon: Some(MirrorFile::get_icon(file_name)),
                    error: None,
                    size: Some(file.metadata().unwrap().len()),
                });
            } else {
                eprintln!("A file was uploaded without a name, skipping.");
                continue;
            }
        }

        if let Some(db) = db {
            if match share.unwrap_or("true") {
                "true" => true,
                "false" => false,
                _ => true,
            } {
                if uploaded_files.len() == 1 {
                    if let Some(mirror_file) = MirrorFileInternal::load_and_share(
                        db,
                        &Path::new(&base_path)
                            .join(&uploaded_files[0].name)
                            .to_path_buf(),
                    )
                    .await
                    {
                        if let Some(id) = mirror_file.id {
                            uploaded_files[0].url = Some(format!("http://{}/share/{}", host.0, id));
                        }
                    }
                }
            }
        }

        return Ok(ApiResponse::UploadFiles(Json(uploaded_files)));
    } else {
        return Err(Status::BadRequest);
    }
}

#[get("/upload")]
async fn upload_info(token: Result<JWT, Status>, sizes: &State<FileSizes>) -> ApiResult {
    let token = token?;

    let upload_limit = *(CONFIG
        .max_upload_sizes
        .get(&token.claims.perms.to_string())
        .unwrap_or(&0_u64));
    let private_folder_quota = *(CONFIG
        .private_folder_quotas
        .get(&token.claims.perms.to_string())
        .unwrap_or(&1_u64));

    let private_folder_usage = sizes
        .read()
        .await
        .iter()
        .find(|entry| {
            entry.file.strip_suffix("/").unwrap_or_default().to_string()
                == format!("files/private/{}", &token.claims.sub)
        })
        .map(|entry| entry.size)
        .unwrap_or(0);

    Ok(ApiResponse::UploadLimits(Json(UploadLimits {
        perms: token.claims.perms,
        upload_limit,
        private_folder_quota,
        private_folder_usage,
    })))
}

#[post("/upload_chunked?<path>&<share>", data = "<data>")]
async fn upload_chunked(
    path: Option<&str>,
    share: Option<&str>,
    content_type: &ContentType,
    data: Data<'_>,
    host: Host<'_>,
    token: Result<JWT, Status>,
    sizes: &State<FileSizes>,
) -> ApiResult {
    perform_upload_chunked(None, path, share, content_type, data, host, token, sizes).await
}

#[post("/upload_chunked?<path>&<share>", data = "<data>")]
async fn upload_chunked_db(
    db: Connection<FileDb>,
    path: Option<&str>,
    share: Option<&str>,
    content_type: &ContentType,
    data: Data<'_>,
    host: Host<'_>,
    token: Result<JWT, Status>,
    sizes: &State<FileSizes>,
) -> ApiResult {
    perform_upload_chunked(
        Some(db),
        path,
        share,
        content_type,
        data,
        host,
        token,
        sizes,
    )
    .await
}

async fn perform_upload_chunked(
    db: Option<Connection<FileDb>>,
    path: Option<&str>,
    share: Option<&str>,
    content_type: &ContentType,
    data: Data<'_>,
    host: Host<'_>,
    token: Result<JWT, Status>,
    sizes: &State<FileSizes>,
) -> ApiResult {
    let token = token?;

    let options = MultipartFormDataOptions::with_multipart_form_data_fields(vec![
        MultipartFormDataField::file("file").size_limit(u64::from(100.megabytes())),
        MultipartFormDataField::text("path"),
        MultipartFormDataField::text("fileid"),
        MultipartFormDataField::text("filename"),
        MultipartFormDataField::text("chunkindex"),
        MultipartFormDataField::text("totalchunks"),
    ]);

    let form_data = MultipartFormData::parse(content_type, data, options)
        .await
        .map_err(|_| Status::BadRequest)?;

    let mut user_path = form_data
        .texts
        .get("path")
        .and_then(|paths| paths.first().map(|p| p.text.trim_matches('/').to_string()))
        .unwrap_or(String::new());

    if user_path.is_empty() {
        user_path = if token.claims.perms == 0 {
            "uploads"
        } else {
            "private"
        }
        .to_string();
    }

    if let Some(query_path) = path {
        user_path = query_path.trim_matches('/').to_string();
    }

    user_path = urlencoding::decode(&user_path)
        .unwrap_or((&user_path).into())
        .to_string();

    let is_private = user_path.starts_with("private");
    if !is_private && token.claims.perms != 0 {
        return Err(Status::Forbidden);
    }

    let base_path = if is_private {
        format!(
            "files/private/{}/{}",
            &token.claims.sub,
            user_path.trim_start_matches("private")
        )
    } else {
        format!("files/{}", user_path)
    };

    let file_id = &form_data.texts["fileid"][0].text;
    let file_name = &form_data.texts["filename"][0].text;
    let chunk_index: usize = form_data.texts["chunkindex"][0]
        .text
        .parse()
        .map_err(|_| Status::BadRequest)?;
    let total_chunks: usize = form_data.texts["totalchunks"][0]
        .text
        .parse()
        .map_err(|_| Status::BadRequest)?;

    let max_size = CONFIG
        .max_upload_sizes
        .get(&token.claims.perms.to_string())
        .unwrap_or(&(104857600 as u64));

    if (total_chunks as u64) * 94371840 > *max_size {
        return Err(Status::PayloadTooLarge);
    }

    let folder_quota = *(CONFIG
        .private_folder_quotas
        .get(&token.claims.perms.to_string())
        .unwrap_or(&1_u64));

    let folder_usage = sizes
        .read()
        .await
        .iter()
        .find(|entry| {
            entry.file.strip_suffix("/").unwrap_or_default().to_string()
                == format!("files/private/{}", &token.claims.sub)
        })
        .map(|entry| entry.size)
        .unwrap_or(0);

    if folder_quota != 0 && folder_usage + ((total_chunks as u64) * 94371840) >= folder_quota {
        return Err(Status::InsufficientStorage);
    }

    let chunk_dir = format!(".chunks/{}/{}", &token.claims.sub, file_id);
    std::fs::create_dir_all(&chunk_dir).map_err(map_io_error_to_status)?;

    let chunk_path = format!("{}/{:05}.part", chunk_dir, chunk_index);

    let file_field = &form_data.files["file"][0];
    let mut src = std::fs::File::open(&file_field.path).map_err(map_io_error_to_status)?;
    let mut dst = std::fs::File::create(&chunk_path).map_err(map_io_error_to_status)?;

    std::io::copy(&mut src, &mut dst).map_err(map_io_error_to_status)?;

    let received_chunks = std::fs::read_dir(&chunk_dir)
        .map_err(map_io_error_to_status)?
        .count();

    if received_chunks < total_chunks {
        return Ok(ApiResponse::UploadFiles(Json(Vec::new())));
    }

    std::fs::create_dir_all(&base_path).map_err(map_io_error_to_status)?;

    let final_path = format!("{}/{}", base_path, file_name);
    let mut final_file = std::fs::File::create(&final_path).map_err(map_io_error_to_status)?;
    let mut final_size: u64 = 0;

    for i in 0..total_chunks {
        let part_path = format!("{}/{:05}.part", chunk_dir, i);
        let mut part = std::fs::File::open(&part_path).map_err(map_io_error_to_status)?;

        let bytes_copied =
            std::io::copy(&mut part, &mut final_file).map_err(map_io_error_to_status)?;

        final_size += bytes_copied;

        if final_size > *max_size {
            drop(final_file);
            let _ = std::fs::remove_file(&final_path);
            let _ = std::fs::remove_dir_all(&chunk_dir);
            return Err(Status::PayloadTooLarge);
        }
    }

    std::fs::remove_dir_all(&chunk_dir).map_err(map_io_error_to_status)?;

    {
        let mut state_lock = sizes.write().await;
        *state_lock = refresh_file_sizes().await;
    }

    if let Some(db) = db {
        if match share.unwrap_or("true") {
            "true" => true,
            "false" => false,
            _ => true,
        } {
            if let Some(mirror_file) = MirrorFileInternal::load_and_share(
                db,
                &Path::new(&base_path).join(&file_name).to_path_buf(),
            )
            .await
            {
                if let Some(id) = mirror_file.id {
                    return Ok(ApiResponse::UploadFiles(Json(vec![UploadFile {
                        name: file_name.clone(),
                        url: Some(format!("http://{}/share/{}", host.0, id)),
                        icon: Some(MirrorFile::get_icon(file_name)),
                        error: None,
                        size: Some(final_file.metadata().unwrap().len()),
                    }])));
                }
            }
        }
    }

    Ok(ApiResponse::UploadFiles(Json(vec![UploadFile {
        name: file_name.clone(),
        url: Some(format!("http://{}/{}/{}", host.0, user_path, file_name)),
        icon: Some(MirrorFile::get_icon(file_name)),
        error: None,
        size: Some(final_file.metadata().unwrap().len()),
    }])))
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
                routes![index, listing, sysinfo, search, upload_info, create_folder,],
            )
            .register("/api", catchers![default]);

        if CONFIG.enable_file_db {
            rocket = rocket.mount(
                "/api",
                routes![
                    file_with_downloads,
                    share,
                    delete_db,
                    rename_db,
                    upload_db,
                    upload_chunked_db
                ],
            )
        } else {
            rocket = rocket.mount(
                "/api",
                routes![file, delete, rename, upload, upload_chunked],
            )
        }

        if CONFIG.enable_zip_downloads {
            rocket = rocket.mount("/api", routes![download_zip])
        }

        rocket
    })
}

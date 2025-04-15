use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use ::sysinfo::{Disks, RefreshKind, System};
use humansize::{format_size, DECIMAL};
use rocket::{
    fairing::AdHoc,
    http::{CookieJar, Status},
    serde::json::Json,
    Request,
};

use crate::{
    read_dirs, read_files,
    utils::{get_session, is_logged_in, is_restricted},
    Config, Disk, MirrorFile, Sysinfo,
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
    if crate::utils::is_logged_in(&jar) {
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
            .mount("/api", routes![index, listing, sysinfo, user])
            .register("/api", catchers![default])
    })
}

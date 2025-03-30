use std::path::{Path, PathBuf};

use ::sysinfo::{Disks, RefreshKind, System};
use humansize::{format_size, DECIMAL};
use rocket::{
    fairing::AdHoc,
    http::{CookieJar, Status},
    serde::json::Json,
    Request,
};
use rocket_dyn_templates::{context, Template};

use crate::{
    read_dirs, read_files,
    utils::{get_bool_cookie, get_theme, is_restricted},
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

    if dir_list.is_empty() {
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
            .mount("/api", routes![index, listing, sysinfo, iframe])
            .register("/api", catchers![default])
    })
}

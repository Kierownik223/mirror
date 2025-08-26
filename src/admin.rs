use std::{
    io::{Read, Write},
    path::Path,
};

use ::sysinfo::{Disks, System};
use rocket::{
    data::ToByteUnit,
    fairing::AdHoc,
    http::{ContentType, CookieJar, Status},
    Data, State,
};
use rocket_dyn_templates::{context, Template};
use rocket_multipart_form_data::{
    MultipartFormData, MultipartFormDataField, MultipartFormDataOptions, Repetition,
};

use crate::{
    utils::{
        format_size, get_bool_cookie, get_extension_from_filename, get_root_domain, get_session, get_theme, is_logged_in
    },
    Config, Disk, Host, Language, MirrorFile, TranslationStore, UsePlain,
};

#[post("/upload", data = "<data>")]
async fn upload(
    content_type: &ContentType,
    data: Data<'_>,
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    config: &State<Config>,
    useplain: UsePlain<'_>,
) -> Result<Template, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
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
                    let normalized_path = file_name.replace('\\', "/");
                    let file_name = &Path::new(&normalized_path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap()
                        .to_string();

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
                                    size: 0,
                                    icon: icon,
                                    downloads: None,
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

            let strings = translations.get_translation(&lang.0);

            return Ok(Template::render(
                if *useplain.0 {
                    "plain/upload"
                } else {
                    "upload"
                },
                context! {
                    title: strings.get("uploader").unwrap(),
                    lang,
                    strings,
                    root_domain: get_root_domain(host.0, &config.fallback_root_domain),
                    host: host.0,
                    config: config.inner(),
                    theme: get_theme(jar),
                    is_logged_in: is_logged_in(jar),
                    hires: get_bool_cookie(jar, "hires", false),
                    smallhead: get_bool_cookie(jar, "smallhead", false),
                    username: username,
                    admin: perms == 0,
                    filebrowser: !get_bool_cookie(jar, "filebrowser", false),
                    uploadedfiles: uploaded_files
                },
            ));
        } else {
            return Err(Status::BadRequest);
        }
    }
}

#[get("/sysinfo")]
fn sysinfo(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    config: &State<Config>,
    useplain: UsePlain<'_>,
) -> Result<Template, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
        let (username, perms) = get_session(jar);

        if perms != 0 {
            return Err(Status::Forbidden);
        }

        let strings = translations.get_translation(&lang.0);

        let mut sys = System::new_all();

        sys.refresh_all();

        let total_mem = sys.total_memory();
        let used_mem = sys.used_memory();

        let sys_name = System::name().unwrap_or(String::from("MARMAK Mirror"));
        let sys_ver = System::kernel_version().unwrap_or(String::from("21.3.7"));
        let hostname = System::host_name().unwrap_or(String::from("mirror"));

        let disks: Vec<Disk> = Disks::new_with_refreshed_list()
            .iter()
            .filter(|x| x.total_space() != 0)
            .map(|x| {
                let used_space = x.total_space() - x.available_space();
                Disk {
                    fs: x.file_system().to_str().unwrap().to_string(),
                    used_space,
                    total_space: x.total_space(),
                    used_space_readable: format_size(used_space),
                    total_space_readable: format_size(x.total_space()),
                }
            })
            .collect();

        return Ok(Template::render(
            if *useplain.0 {
                "plain/sysinfo"
            } else {
                "sysinfo"
            },
            context! {
                title: strings.get("sysinfo").unwrap(),
                lang,
                strings,
                root_domain: get_root_domain(host.0, &config.fallback_root_domain),
                host: host.0,
                config: config.inner(),
                theme: get_theme(jar),
                is_logged_in: is_logged_in(jar),
                hires: get_bool_cookie(jar, "hires", false),
                admin: get_session(jar).1 == 0,
                smallhead: get_bool_cookie(jar, "smallhead", false),
                username: username,
                total_mem: total_mem,
                total_mem_readable: format_size(total_mem),
                used_mem: used_mem,
                used_mem_readable: format_size(used_mem),
                sys_name: sys_name,
                sys_ver: sys_ver,
                hostname: hostname,
                disks: disks
            },
        ));
    }
}

#[get("/upload")]
fn uploader(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    config: &State<Config>,
    useplain: UsePlain<'_>,
) -> Result<Template, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
        let (username, perms) = get_session(jar);

        if perms != 0 {
            return Err(Status::Forbidden);
        }

        let strings = translations.get_translation(&lang.0);

        return Ok(Template::render(
            if *useplain.0 {
                "plain/upload"
            } else {
                "upload"
            },
            context! {
                title: strings.get("uploader").unwrap(),
                lang,
                strings,
                root_domain: get_root_domain(host.0, &config.fallback_root_domain),
                host: host.0,
                config: config.inner(),
                theme: get_theme(jar),
                is_logged_in: is_logged_in(jar),
                hires: get_bool_cookie(jar, "hires", false),
                smallhead: get_bool_cookie(jar, "smallhead", false),
                username: username,
                admin: perms == 0,
                filebrowser: !get_bool_cookie(jar, "filebrowser", false),
                uploadedfiles: vec![MirrorFile { name: "".to_string(), ext: "".to_string(), icon: "default".to_string(), size: 0, downloads: None }]
            },
        ));
    }
}

#[get("/")]
fn admin(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    config: &State<Config>,
    useplain: UsePlain<'_>,
) -> Result<Template, Status> {
    if !is_logged_in(jar) {
        return Err(Status::Unauthorized);
    } else {
        let (username, perms) = get_session(jar);

        if perms != 0 {
            return Err(Status::Forbidden);
        }

        let strings = translations.get_translation(&lang.0);

        return Ok(Template::render(
            if *useplain.0 { "plain/admin" } else { "admin" },
            context! {
                title: strings.get("admin").unwrap(),
                lang,
                strings,
                root_domain: get_root_domain(host.0, &config.fallback_root_domain),
                host: host.0,
                config: config.inner(),
                theme: get_theme(jar),
                is_logged_in: is_logged_in(jar),
                hires: get_bool_cookie(jar, "hires", false),
                smallhead: get_bool_cookie(jar, "smallhead", false),
                username: username,
                admin: perms == 0,
            },
        ));
    }
}
pub fn build() -> AdHoc {
    AdHoc::on_ignite("Admin", |rocket| async {
        rocket.mount("/admin", routes![upload, uploader, sysinfo, admin])
    })
}

use ::sysinfo::{Disks, System};
use rocket::{
    fairing::AdHoc,
    http::{CookieJar, Status},
};
use rocket_dyn_templates::{context, Template};

use crate::{
    config::CONFIG,
    jwt::JWT,
    utils::{format_size, get_bool_cookie, get_root_domain, get_theme},
    Disk, Host, IndexResponse, Language, TranslationStore, UsePlain,
};

#[get("/sysinfo")]
fn sysinfo(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    useplain: UsePlain<'_>,
    token: Result<JWT, Status>,
) -> Result<IndexResponse, Status> {
    let token = token?;

    let username = token.claims.sub;
    let perms = token.claims.perms;

    if perms != 0 {
        return Err(Status::Forbidden);
    }

    let use_si = get_bool_cookie(jar, "use_si", true);

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
                fs: x.file_system().to_str().unwrap_or_default().to_string(),
                used_space,
                total_space: x.total_space(),
                used_space_readable: format_size(used_space, use_si),
                total_space_readable: format_size(x.total_space(), use_si),
                mount_point: x.mount_point().display().to_string(),
            }
        })
        .collect();

    return Ok(IndexResponse::Template(Template::render(
        if *useplain.0 {
            "plain/sysinfo"
        } else {
            "sysinfo"
        },
        context! {
            title: strings.get("sysinfo").unwrap_or(&("sysinfo".into())),
            lang,
            strings,
            root_domain: get_root_domain(host.0),
            host: host.0,
            config: (*CONFIG).clone(),
            theme: get_theme(jar),
            is_logged_in: true,
            hires: get_bool_cookie(jar, "hires", false),
            admin: perms == 0,
            smallhead: get_bool_cookie(jar, "smallhead", false),
            username: username,
            total_mem: total_mem,
            total_mem_readable: format_size(total_mem, use_si),
            used_mem: used_mem,
            used_mem_readable: format_size(used_mem, use_si),
            sys_name: sys_name,
            sys_ver: sys_ver,
            hostname: hostname,
            disks: disks
        },
    )));
}

#[get("/")]
fn admin(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    useplain: UsePlain<'_>,
    token: Result<JWT, Status>,
) -> Result<IndexResponse, Status> {
    let token = token?;

    let username = token.claims.sub;
    let perms = token.claims.perms;

    if perms != 0 {
        return Err(Status::Forbidden);
    }

    let strings = translations.get_translation(&lang.0);

    return Ok(IndexResponse::Template(Template::render(
        if *useplain.0 { "plain/admin" } else { "admin" },
        context! {
            title: strings.get("admin").unwrap_or(&("admin".into())),
            lang,
            strings,
            root_domain: get_root_domain(host.0),
            host: host.0,
            config: (*CONFIG).clone(),
            theme: get_theme(jar),
            is_logged_in: true,
            hires: get_bool_cookie(jar, "hires", false),
            smallhead: get_bool_cookie(jar, "smallhead", false),
            username: username,
            admin: perms == 0,
        },
    )));
}

pub fn build() -> AdHoc {
    AdHoc::on_ignite("Admin", |rocket| async {
        rocket.mount("/admin", routes![sysinfo, admin])
    })
}

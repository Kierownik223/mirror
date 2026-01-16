use ::sysinfo::{Disks, System};
use rocket::{
    fairing::AdHoc,
    http::{CookieJar, Status},
};
use rocket_dyn_templates::{context, Template};

use crate::{
    config::CONFIG,
    guards::Settings,
    jwt::JWT,
    responders::IndexResult,
    utils::{format_size, get_root_domain},
    Disk, Host, IndexResponse, Language, TranslationStore,
};

#[get("/sysinfo")]
fn sysinfo(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    token: Result<JWT, Status>,
    settings: Settings<'_>,
) -> IndexResult {
    let token = token?;

    if let Some(t) = token.token {
        let mut jwt_cookie = rocket::http::Cookie::new("matoken", t.to_string());
        jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
        jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(jwt_cookie);

        let mut local_jwt_cookie = rocket::http::Cookie::new("token", t.to_string());
        local_jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(local_jwt_cookie);
    }

    if !::sysinfo::IS_SUPPORTED_SYSTEM {
        return Err(Status::NotFound);
    }

    let username = token.claims.sub;
    let perms = token.claims.perms;

    if perms != 0 {
        return Err(Status::Forbidden);
    }

    let strings = translations.get_translation(&lang.0);

    let disks: Vec<Disk> = Disks::new_with_refreshed_list()
        .iter()
        .filter(|x| x.total_space() != 0)
        .map(|x| {
            let used_space = x.total_space() - x.available_space();
            Disk {
                fs: x.file_system().to_str().unwrap_or("unknown").to_string(),
                used_space,
                total_space: x.total_space(),
                used_space_readable: format_size(used_space, settings.use_si),
                total_space_readable: format_size(x.total_space(), settings.use_si),
                mount_point: x.mount_point().display().to_string(),
            }
        })
        .collect();

    return Ok(IndexResponse::Template(Template::render(
        if settings.plain {
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
            is_logged_in: true,
            admin: perms == 0,
            username,
            system: System::new_all(),
            hostname: System::host_name(),
            sys_name: System::name(),
            sys_ver: System::kernel_version(),
            disks,
            settings,
        },
    )));
}

#[get("/")]
fn admin(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    token: Result<JWT, Status>,
    settings: Settings<'_>,
) -> IndexResult {
    let token = token?;

    if let Some(t) = token.token {
        let mut jwt_cookie = rocket::http::Cookie::new("matoken", t.to_string());
        jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
        jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(jwt_cookie);

        let mut local_jwt_cookie = rocket::http::Cookie::new("token", t.to_string());
        local_jwt_cookie.set_same_site(rocket::http::SameSite::Lax);

        jar.add(local_jwt_cookie);
    }

    let username = token.claims.sub;
    let perms = token.claims.perms;

    if perms != 0 {
        return Err(Status::Forbidden);
    }

    let strings = translations.get_translation(&lang.0);

    return Ok(IndexResponse::Template(Template::render(
        if settings.plain { "plain/admin" } else { "admin" },
        context! {
            title: strings.get("admin").unwrap_or(&("admin".into())),
            lang,
            strings,
            root_domain: get_root_domain(host.0),
            host: host.0,
            config: (*CONFIG).clone(),
            is_logged_in: true,
            username: username,
            admin: perms == 0,
            settings,
        },
    )));
}

pub fn build() -> AdHoc {
    AdHoc::on_ignite("Admin", |rocket| async {
        rocket.mount("/admin", routes![sysinfo, admin])
    })
}

use std::{collections::HashMap, fs};

use base64::{prelude::BASE64_STANDARD, Engine};
use openssl::rsa::{Padding, Rsa};
use rocket::{
    fairing::AdHoc,
    form::Form,
    http::{Cookie, CookieJar, Status},
    response::Redirect, State,
};
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};
use serde_json::json;
use time::{Duration, OffsetDateTime};

use crate::{
    db::{fetch_user, login_user, Db}, utils::{get_bool_cookie, get_session, get_theme, is_logged_in}, Config, Host, Language, MarmakUser, TranslationStore, UserToken, XForwardedFor
};

#[get("/login")]
fn login_page(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    config: &State<Config>
) -> Result<Template, Redirect> {
    if is_logged_in(&jar) {
        let perms = get_session(jar).1;
        if perms == 0 {
            return Err(Redirect::to("/admin/"));
        } else {
            return Err(Redirect::to("/"));
        }
    }

    let strings = translations.get_translation(&lang.0);

    let root_domain = host.0.splitn(2, '.').nth(1).unwrap_or("marmak.net.pl");

    Ok(Template::render(
        "login",
        context! {
            title: "Login",
            lang,
            strings,
            root_domain,
            login: config.enable_login,
            marmak_link: config.enable_marmak_link,
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
    translations: &State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    config: &State<Config>
) -> Result<Redirect, Template> {
    if let Some(db_user) = login_user(db, &user.username, &user.password, &ip.0, true).await {
        if !get_bool_cookie(&jar, "nooverride") {
            if let Some(mirror_settings) = db_user.mirror_settings {
                let decoded: HashMap<String, String> =
                    serde_json::from_str(&mirror_settings).unwrap_or_default();

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
                &db_user.username,
                &db_user.perms.unwrap_or_default().to_string()
            ),
        ));

        println!(
            "Login for user {} from {} succeeded",
            &db_user.username, &ip.0
        );

        let mut redirect_url = next.unwrap_or("/");

        if redirect_url == "/admin" {
            return Ok(Redirect::to("/"));
        }

        if db_user.perms.unwrap_or(1) == 0 {
            redirect_url = next.unwrap_or("/admin");
        }

        return Ok(Redirect::to(redirect_url.replace(" ", "%20")));
    } else {
        let strings = translations.get_translation(&lang.0);

        let root_domain = host.0.splitn(2, '.').nth(1).unwrap_or("marmak.net.pl");

        println!(
            "Failed login attempt to user {} with password {} from {}",
            &user.username, &user.password, &ip.0
        );

        return Err(Template::render(
            "login",
            context! {
                title: "Login",
                lang,
                strings,
                root_domain,
                login: config.enable_login,
                marmak_link: config.enable_marmak_link,
                theme: get_theme(jar),
                is_logged_in: is_logged_in(&jar),
                admin: get_session(&jar).1 == 0,
                hires: get_bool_cookie(jar, "hires"),
                smallhead: get_bool_cookie(jar, "smallhead"),
                message: strings.get("invalid_info")
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
    host: Host<'_>,
) -> Result<Redirect, Status> {
    if let Some(token) = token {
        if is_logged_in(&jar) {
            let perms = get_session(jar).1;
            return Ok(Redirect::to(if perms == 0 { "/admin" } else { "/" }));
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
                        serde_json::from_str(&mirror_settings).unwrap_or_default();

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
            return Ok(Redirect::to("/"));
        }

        return Ok(Redirect::to("/account/login"));
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

                let root_domain = host.0.splitn(2, '.').nth(1).unwrap_or("marmak.net.pl");

                let redirect_url = format!(
                    "http://account.{}/direct?token={}",
                    root_domain, encrypted_b64
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

#[get("/logout")]
fn logout(jar: &CookieJar<'_>) -> Redirect {
    jar.remove_private("session");
    Redirect::to("/account/login")
}

pub fn build_account() -> AdHoc {
    AdHoc::on_ignite("Account", |rocket| async {
        rocket.mount("/account", routes![login_page, login, direct, logout])
    })
}

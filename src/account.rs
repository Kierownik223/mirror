use std::{collections::HashMap, fs};

use base64::{prelude::BASE64_STANDARD, Engine};
use rand::thread_rng;
use rocket::{
    fairing::AdHoc,
    form::Form,
    http::{Cookie, CookieJar, Status},
    response::Redirect,
    State,
};
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};
use rsa::{RsaPrivateKey, RsaPublicKey};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs1v15::Pkcs1v15Encrypt;
use serde_json::json;
use time::{Duration, OffsetDateTime};

use crate::{
    db::{fetch_user, login_user, Db},
    utils::{get_bool_cookie, get_session, get_theme, is_logged_in},
    Config, Host, Language, MarmakUser, TranslationStore, UsePlain, UserToken, XForwardedFor,
};

#[get("/login")]
fn login_page(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    config: &State<Config>,
    useplain: UsePlain<'_>,
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
        if *useplain.0 { "plain/login" } else { "login" },
        context! {
            title: "Login",
            lang,
            strings,
            root_domain,
            host: host.0,
            config: config.inner(),
            theme: get_theme(jar),
            is_logged_in: is_logged_in(&jar),
            username: "",
            admin: false,
            hires: get_bool_cookie(jar, "hires", false),
            smallhead: get_bool_cookie(jar, "smallhead", false),
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
    config: &State<Config>,
    useplain: UsePlain<'_>,
) -> Result<Redirect, Template> {
    if let Some(db_user) = login_user(db, &user.username, &user.password, &ip.0, true).await {
        if !get_bool_cookie(&jar, "nooverride", false) {
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
            if *useplain.0 { "plain/login" } else { "login" },
            context! {
                title: "Login",
                lang,
                strings,
                root_domain,
                host: host.0,
                config: config.inner(),
                theme: get_theme(jar),
                is_logged_in: is_logged_in(&jar),
                admin: get_session(&jar).1 == 0,
                hires: get_bool_cookie(jar, "hires", false),
                smallhead: get_bool_cookie(jar, "smallhead", false),
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

        let private_key_pem = fs::read_to_string("private.key").map_err(|_| Status::InternalServerError)?;
        let private_key = RsaPrivateKey::from_pkcs1_pem(&private_key_pem).map_err(|_| Status::InternalServerError)?;

        let encrypted_data = base64::engine::general_purpose::URL_SAFE
            .decode(&token.replace(".", "="))
            .map_err(|_| Status::BadRequest)?;

        let decrypted_data = private_key.decrypt(Pkcs1v15Encrypt, &encrypted_data).map_err(|_| Status::InternalServerError)?;

        let mut json_bytes = Vec::new();
        BASE64_STANDARD
            .decode_vec(&decrypted_data, &mut json_bytes)
            .map_err(|_| Status::BadRequest)?;

        let json = String::from_utf8(json_bytes).map_err(|_| Status::InternalServerError)?;
        let received_user: UserToken =
            serde_json::from_str(&json).map_err(|_| Status::BadRequest)?;

        if let Some(db_user) = login_user(db, &received_user.username, "", ip.0, false).await {
            if !get_bool_cookie(&jar, "nooverride", false) {
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

    if let Some(to) = to {
        if !is_logged_in(&jar) {
            return Err(Status::Unauthorized);
        } else {
            if let Some(db_user) = fetch_user(db, get_session(jar).0.as_str()).await {
                let user_data =
                    json!({"username": get_session(jar).0, "password_hash": db_user.password});
                let b64token = BASE64_STANDARD.encode(user_data.to_string());

                let public_key_pem = fs::read_to_string("public.key").map_err(|_| Status::InternalServerError)?;
                let public_key = RsaPublicKey::from_pkcs1_pem(&public_key_pem).map_err(|_| Status::InternalServerError)?;

                let mut rng = thread_rng();
                let encrypted_data = public_key.encrypt(&mut rng, Pkcs1v15Encrypt, b64token.as_bytes())
                    .map_err(|_| Status::InternalServerError)?;

                let encrypted_b64 =
                    base64::engine::general_purpose::URL_SAFE.encode(encrypted_data);

                let root_domain = host.0.splitn(2, '.').nth(1).unwrap_or("marmak.net.pl");

                let redirect_url = format!(
                    "http://{}/direct?token={}",
                    match to.as_str() {
                        "account" => format!("account.{}", root_domain),
                        "marmak" => root_domain.to_string(),
                        "karol" => format!("karol.{}", root_domain),
                        _ => host.0.to_string(),
                    },
                    encrypted_b64
                );

                return Ok(Redirect::to(redirect_url));
            } else {
                return Err(Status::Forbidden);
            }
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
        let config = Config::load();

        let mut rocket = rocket.mount("/account", routes![login_page, login, logout]);

        if config.enable_direct {
            rocket = rocket.mount("/account", routes![direct]);
        }

        rocket
    })
}

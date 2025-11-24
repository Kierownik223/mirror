use std::{collections::HashMap, fs, path::Path};

use base64::{prelude::BASE64_STANDARD, Engine};
use rand::thread_rng;
use rocket::{
    fairing::AdHoc,
    form::Form,
    http::{Cookie, CookieJar, SameSite, Status},
    response::Redirect,
    State,
};
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};
use rsa::pkcs1::{DecodeRsaPrivateKey, DecodeRsaPublicKey};
use rsa::pkcs1v15::Pkcs1v15Encrypt;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::json;
use time::{Duration, OffsetDateTime};

use crate::{
    Host, IndexResponse, Language, TranslationStore, UsePlain, config::CONFIG, db::{Db, add_rememberme_token, delete_session, fetch_user, login_user}, guards::XForwardedFor, jwt::{JWT, create_jwt}, utils::{get_bool_cookie, get_root_domain, get_theme, map_io_error_to_status}
};

#[derive(Debug, PartialEq, Eq, FromForm)]
pub struct MarmakUser {
    pub username: String,
    pub password: String,
    pub perms: i32,
    pub mirror_settings: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, PartialEq, Eq, FromForm)]
struct LoginUser {
    username: String,
    password: String,
    remember_me: Option<bool>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct UserToken {
    username: String,
    password_hash: String,
}

#[get("/login?<next>")]
fn login_page(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    useplain: UsePlain<'_>,
    next: Option<&str>,
    token: Result<JWT, Status>,
) -> IndexResponse {
    if let Ok(token) = token {
        let perms = token.claims.perms;

        if perms == 0 {
            return IndexResponse::Redirect(Redirect::to("/admin/"));
        } else {
            return IndexResponse::Redirect(Redirect::to("/"));
        }
    }

    let strings = translations.get_translation(&lang.0);

    let next = next.unwrap_or("");

    IndexResponse::Template(Template::render(
        if *useplain.0 { "plain/login" } else { "login" },
        context! {
            title: "Login",
            lang,
            strings,
            root_domain: get_root_domain(host.0),
            host: host.0,
            config: (*CONFIG).clone(),
            theme: get_theme(jar),
            is_logged_in: token.is_ok(),
            username: "",
            admin: false,
            hires: get_bool_cookie(jar, "hires", false),
            smallhead: get_bool_cookie(jar, "smallhead", false),
            message: "",
            next
        },
    ))
}

#[post("/login?<next>", data = "<user>")]
async fn login(
    db: Connection<Db>,
    db2: Connection<Db>,
    user: Form<LoginUser>,
    jar: &CookieJar<'_>,
    ip: XForwardedFor<'_>,
    next: Option<&str>,
    translations: &State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    useplain: UsePlain<'_>,
    token: Result<JWT, Status>,
) -> Result<IndexResponse, Status> {
    if let Some(db_user) = login_user(db, &user.username, &user.password, &ip.0, true).await {
        if !get_bool_cookie(jar, "nooverride", false) {
            if let Some(mirror_settings) = db_user.mirror_settings.as_ref() {
                let decoded: HashMap<String, String> =
                    serde_json::from_str(&mirror_settings).unwrap_or_default();

                for (key, value) in decoded {
                    let mut now = OffsetDateTime::now_utc();
                    now += Duration::days(365);
                    let mut cookie = Cookie::new(key, value);
                    cookie.set_expires(now);
                    cookie.set_same_site(SameSite::Lax);
                    jar.add(cookie);
                }
            }
        }

        if let Some(_) = user.remember_me {
            println!("a");
            let rememberme_token = add_rememberme_token(db2, &db_user.username).await;

            let month = OffsetDateTime::now_utc() + Duration::days(30);

            let mut rememberme_cookie = Cookie::new("maremembertoken", rememberme_token.clone());
            rememberme_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
            rememberme_cookie.set_expires(month);
            rememberme_cookie.set_same_site(SameSite::Lax);

            jar.add(rememberme_cookie);

            let mut local_rememberme_cookie =
                Cookie::new("remembermetoken", rememberme_token.clone());
            local_rememberme_cookie.set_expires(month);
            local_rememberme_cookie.set_same_site(SameSite::Lax);

            jar.add(local_rememberme_cookie);
        }

        let jwt = create_jwt(&db_user).map_err(|_| Status::InternalServerError)?;

        let mut jwt_cookie = Cookie::new("matoken", jwt.clone());
        jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
        jwt_cookie.set_same_site(SameSite::Lax);

        jar.add(jwt_cookie);

        let mut local_jwt_cookie = Cookie::new("token", jwt.clone());
        local_jwt_cookie.set_same_site(SameSite::Lax);

        jar.add(local_jwt_cookie);

        println!(
            "Login for user {} from {} succeeded",
            &db_user.username, &ip.0
        );

        if !Path::new(&format!("files/private/{}", &db_user.username)).exists() {
            let _ = fs::create_dir(format!("files/private/{}", &db_user.username));
        }

        let mut redirect_url = next.unwrap_or("/");

        if db_user.perms == 0 {
            redirect_url = next.unwrap_or("/admin");
        }

        Ok(IndexResponse::Redirect(Redirect::to(
            urlencoding::encode(redirect_url).replace("%2F", "/"),
        )))
    } else {
        let strings = translations.get_translation(&lang.0);

        println!(
            "Failed login attempt to user {} with from {}",
            &user.username, &ip.0
        );

        Ok(IndexResponse::Template(Template::render(
            if *useplain.0 { "plain/login" } else { "login" },
            context! {
                title: "Login",
                lang,
                strings,
                root_domain: get_root_domain(host.0),
                host: host.0,
                config: (*CONFIG).clone(),
                theme: get_theme(jar),
                is_logged_in: token.is_ok(),
                admin: token.unwrap_or_default().claims.perms == 0,
                hires: get_bool_cookie(jar, "hires", false),
                smallhead: get_bool_cookie(jar, "smallhead", false),
                message: strings.get("invalid_info"),
                next: "",
            },
        )))
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
    jwt: Result<JWT, Status>,
) -> Result<Redirect, Status> {
    if let Some(token) = token {
        let private_key_pem = fs::read_to_string("private.key").map_err(map_io_error_to_status)?;
        let private_key =
            RsaPrivateKey::from_pkcs1_pem(&private_key_pem).expect("Failed to create private_key");

        let encrypted_data = base64::engine::general_purpose::URL_SAFE
            .decode(&token.replace(".", "="))
            .map_err(|_| Status::BadRequest)?;

        let decrypted_data = private_key
            .decrypt(Pkcs1v15Encrypt, &encrypted_data)
            .expect("Failed to decrypt payload");

        let mut json_bytes = Vec::new();
        BASE64_STANDARD
            .decode_vec(&decrypted_data, &mut json_bytes)
            .map_err(|_| Status::BadRequest)?;

        let json = String::from_utf8(json_bytes).expect("Failed to get payload string");
        let received_user: UserToken =
            serde_json::from_str(&json).map_err(|_| Status::BadRequest)?;

        if let Some(db_user) = login_user(db, &received_user.username, "", ip.0, false).await {
            if !get_bool_cookie(jar, "nooverride", false) {
                if let Some(mirror_settings) = db_user.mirror_settings.as_ref() {
                    let decoded: HashMap<String, String> =
                        serde_json::from_str(&mirror_settings).unwrap_or_default();

                    for (key, value) in decoded {
                        let year = OffsetDateTime::now_utc() + Duration::days(365);
                        let mut cookie = Cookie::new(key, value.to_string());
                        cookie.set_expires(year);
                        cookie.set_same_site(SameSite::Lax);
                    }
                }
            }

            let jwt = create_jwt(&db_user).map_err(|_| Status::InternalServerError)?;

            let mut jwt_cookie = Cookie::new("matoken", jwt.clone());
            jwt_cookie.set_domain(format!(".{}", get_root_domain(host.0)));
            jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(jwt_cookie);

            let mut local_jwt_cookie = Cookie::new("token", jwt.clone());
            local_jwt_cookie.set_same_site(SameSite::Lax);

            jar.add(local_jwt_cookie);

            if !Path::new(&format!("files/private/{}", &db_user.username)).exists() {
                let _ = fs::create_dir(format!("files/private/{}", &db_user.username));
            }

            return Ok(Redirect::to("/"));
        }

        return Ok(Redirect::to("/account/login"));
    }

    if let Some(to) = to {
        if jwt.is_err() {
            return Err(Status::Unauthorized);
        } else {
            let token = jwt?;
            if let Some(db_user) = fetch_user(db, &token.claims.sub).await {
                let user_data =
                    json!({"username": &token.claims.sub, "password_hash": db_user.password});
                let b64token = BASE64_STANDARD.encode(user_data.to_string());

                let public_key_pem =
                    fs::read_to_string("public.key").map_err(|_| Status::InternalServerError)?;
                let public_key = RsaPublicKey::from_pkcs1_pem(&public_key_pem)
                    .map_err(|_| Status::InternalServerError)?;

                let mut rng = thread_rng();
                let encrypted_data = public_key
                    .encrypt(&mut rng, Pkcs1v15Encrypt, b64token.as_bytes())
                    .map_err(|_| Status::InternalServerError)?;

                let encrypted_b64 =
                    base64::engine::general_purpose::URL_SAFE.encode(encrypted_data);

                let root_domain = get_root_domain(host.0);

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
                jar.remove(
                    Cookie::build("matoken")
                        .domain(format!(".{}", get_root_domain(host.0)))
                        .same_site(SameSite::Lax),
                );
                return Err(Status::Forbidden);
            }
        }
    }

    Err(Status::BadRequest)
}

#[get("/logout")]
async fn logout(db: Connection<Db>, db2: Connection<Db>, jar: &CookieJar<'_>, host: Host<'_>) -> Redirect {
    jar.remove(
        Cookie::build("matoken")
            .domain(format!(".{}", get_root_domain(host.0)))
            .same_site(SameSite::Lax),
    );
    jar.remove(Cookie::build("token").same_site(SameSite::Lax));

    if jar.get("maremembermetoken").is_some() {
        delete_session(db, jar.get("maremembermetoken").unwrap().value()).await;

        jar.remove(
            Cookie::build("maremembermetoken")
                .domain(format!(".{}", get_root_domain(host.0)))
                .same_site(SameSite::Lax),
        );
    }
    if jar.get("remembermetoken").is_some() {
        delete_session(db2, jar.get("remembermetoken").unwrap().value()).await;

        jar.remove(
            Cookie::build("remembermetoken").same_site(SameSite::Lax),
        );
    }
    Redirect::to("/account/login")
}

pub fn build_account() -> AdHoc {
    AdHoc::on_ignite("Account", |rocket| async {
        let mut rocket = rocket.mount("/account", routes![login_page, login, logout]);

        if CONFIG.enable_direct {
            rocket = rocket.mount("/account", routes![direct]);
        }

        rocket
    })
}

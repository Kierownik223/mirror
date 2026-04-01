use std::{collections::HashMap, fs, path::Path};

use bcrypt::verify;
use rocket::{
    fairing::AdHoc,
    form::Form,
    http::{Cookie, CookieJar, SameSite, Status},
    response::Redirect,
    time::{Duration, OffsetDateTime},
    State,
};
use rocket_db_pools::{
    sqlx::{self, Row},
    Connection,
};
use rocket_dyn_templates::{context, Template};

use crate::{
    config::CONFIG,
    db::{add_login, add_rememberme_token, delete_session, Db},
    guards::{Settings, XForwardedFor},
    jwt::{create_jwt, JWT},
    responders::IndexResult,
    utils::{add_token_cookie, get_root_domain},
    Host, IndexResponse, Language, TranslationStore,
};

#[derive(Debug, PartialEq, Eq, FromForm)]
pub struct MarmakUser {
    pub username: String,
    pub password: String,
    pub perms: i32,
    pub mirror_settings: Option<String>,
    pub email: Option<String>,
}

impl MarmakUser {
    pub async fn login(
        mut db: Connection<Db>,
        username: &str,
        password: &str,
        ip: &str,
    ) -> Option<Self> {
        let query_result = sqlx::query(
            "SELECT username, password, perms, mirror_settings, email FROM users WHERE username = ? AND verified = 1",
        )
        .bind(username)
        .fetch_one(&mut **db)
        .await;

        if username == "Nobody" {
            return None;
        }

        match query_result {
            Ok(row) => {
                let stored_hash = row.try_get::<String, _>("password").ok()?;
                let username = row.try_get::<String, _>("username").ok()?;
                if verify(password, &stored_hash).unwrap_or(false) {
                    let perms = row.try_get::<i32, _>("perms").ok()?;

                    add_login(db, username.as_str(), ip).await;

                    return Some(Self {
                        username: username,
                        password: password.to_string(),
                        perms,
                        mirror_settings: row.try_get::<String, _>("mirror_settings").ok(),
                        email: row.try_get::<String, _>("email").ok(),
                    });
                } else {
                    None
                }
            }
            Err(error) => {
                eprintln!("Database error (login_user): {:?}", error);
                None
            }
        }
    }

    pub async fn get(mut db: Connection<Db>, username: &str) -> Option<MarmakUser> {
        let query_result = sqlx::query(
            "SELECT username, password, perms, mirror_settings, email FROM users WHERE username = ? AND verified = 1",
        )
        .bind(username)
        .fetch_one(&mut **db)
        .await;

        if username == "Nobody" {
            return None;
        }

        match query_result {
            Ok(row) => {
                let perms = row.try_get::<i32, _>("perms").ok()?;

                return Some(MarmakUser {
                    username: row
                        .try_get::<String, _>("username")
                        .ok()
                        .unwrap_or_default(),
                    password: row
                        .try_get::<String, _>("password")
                        .ok()
                        .unwrap_or_default(),
                    perms,
                    mirror_settings: row.try_get::<String, _>("mirror_settings").ok(),
                    email: row.try_get::<String, _>("email").ok(),
                });
            }
            Err(error) => {
                eprintln!("Database error (get_user): {:?}", error);
                None
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, FromForm)]
struct LoginUser {
    username: String,
    password: String,
    remember_me: Option<bool>,
}

#[get("/login?<next>")]
fn login_page(
    jar: &CookieJar<'_>,
    translations: &rocket::State<TranslationStore>,
    lang: Language,
    host: Host<'_>,
    next: Option<&str>,
    token: Result<JWT, Status>,
    settings: Settings<'_>,
) -> IndexResponse {
    if let Ok(token) = token {
        if let Some(t) = &token.token {
            add_token_cookie(&t, &host.0, jar);
        }

        if token.claims.perms == 0 {
            return IndexResponse::Redirect(Redirect::to("/admin/"));
        } else {
            return IndexResponse::Redirect(Redirect::to("/"));
        }
    }

    let strings = translations.get_translation(&lang.0);

    IndexResponse::Template(Template::render(
        if settings.plain {
            "plain/login"
        } else {
            "login"
        },
        context! {
            title: strings.get("log_in"),
            lang,
            strings,
            root_domain: get_root_domain(host.0),
            host: host.0,
            config: (*CONFIG).clone(),
            next,
            settings,
            version: env!("CARGO_PKG_VERSION").to_string(),
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
    settings: Settings<'_>,
) -> IndexResult {
    if let Some(db_user) = MarmakUser::login(db, &user.username, &user.password, &ip.0).await {
        if !settings.nooverride {
            if let Some(mirror_settings) = db_user.mirror_settings.as_ref() {
                let decoded: HashMap<String, String> =
                    serde_json::from_str(&mirror_settings).unwrap_or_default();

                Settings::from_hashmap(&decoded).to_cookies(jar);
            }
        }

        if let Some(_) = user.remember_me {
            if let Some(rememberme_token) = add_rememberme_token(db2, &db_user.username).await {
                let month = OffsetDateTime::now_utc() + Duration::days(30);

                let mut rememberme_cookie =
                    Cookie::new("maremembermetoken", rememberme_token.clone());
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
        }

        let jwt = create_jwt(&db_user).map_err(|_| Status::InternalServerError)?;

        add_token_cookie(&jwt, &host.0, jar);

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
            if settings.plain {
                "plain/login"
            } else {
                "login"
            },
            context! {
                title: strings.get("log_in"),
                lang,
                strings,
                root_domain: get_root_domain(host.0),
                host: host.0,
                config: (*CONFIG).clone(),
                message: strings.get("invalid_info"),
                next,
                settings,
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        )))
    }
}

#[get("/direct")]
fn direct(jwt: Result<JWT, Status>) -> Redirect {
    if jwt.is_ok() {
        Redirect::to("/")
    } else {
        Redirect::to("/account/login")
    }
}

#[get("/logout")]
async fn logout(
    db: Connection<Db>,
    db2: Connection<Db>,
    jar: &CookieJar<'_>,
    host: Host<'_>,
) -> Redirect {
    jar.remove(
        Cookie::build("matoken")
            .domain(format!(".{}", get_root_domain(host.0)))
            .same_site(SameSite::Lax),
    );
    jar.remove(Cookie::build("token").same_site(SameSite::Lax));

    if let Some(cookie) = jar.get("maremembermetoken") {
        delete_session(db, cookie.value()).await;

        jar.remove(
            Cookie::build("maremembermetoken")
                .domain(format!(".{}", get_root_domain(host.0)))
                .same_site(SameSite::Lax),
        );
    }
    if let Some(cookie) = jar.get("remembermetoken") {
        delete_session(db2, cookie.value()).await;

        jar.remove(Cookie::build("remembermetoken").same_site(SameSite::Lax));
    }
    Redirect::to("/account/login")
}

pub fn build_account() -> AdHoc {
    AdHoc::on_ignite("Account", |rocket| async {
        rocket.mount("/account", routes![login_page, login, logout, direct])
    })
}

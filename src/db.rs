use rocket_db_pools::{sqlx, Connection, Database};
use sqlx::Row;

use bcrypt::verify;
use uuid::Uuid;

use crate::MarmakUser;

#[derive(Database)]
#[database("marmak")]
pub struct Db(sqlx::MySqlPool);

#[derive(Database)]
#[database("mirror")]
pub struct FileDb(sqlx::MySqlPool);

pub async fn login_user(
    mut db: Connection<Db>,
    username: &str,
    password: &str,
    ip: &str,
    verify_password: bool,
) -> Option<MarmakUser> {
    let query_result = sqlx::query(
        "SELECT username, password, perms, mirror_settings, email FROM users WHERE username = ? AND verified = 1",
    )
    .bind(username)
    .fetch_one(&mut **db)
    .await;

    match query_result {
        Ok(row) => {
            if let Some(stored_hash) = row.try_get::<String, _>("password").ok() {
                let username = row.try_get::<String, _>("username").ok().unwrap();
                if verify_password && verify(password, &stored_hash).unwrap_or(false) {
                    if let Some(perms) = row.try_get::<i32, _>("perms").ok() {
                        add_login(db, username.as_str(), ip).await;
                        let settings = row.try_get::<String, _>("mirror_settings").ok();
                        return Some(MarmakUser {
                            username: username,
                            password: password.to_string(),
                            perms: perms,
                            mirror_settings: settings,
                            email: row.try_get::<String, _>("email").ok(),
                        });
                    } else {
                        None
                    }
                } else if !verify_password {
                    if let Some(perms) = row.try_get::<i32, _>("perms").ok() {
                        add_login(db, username.as_str(), ip).await;
                        let settings = row.try_get::<String, _>("mirror_settings").ok();
                        return Some(MarmakUser {
                            username: username,
                            password: password.to_string(),
                            perms: perms,
                            mirror_settings: settings,
                            email: row.try_get::<String, _>("email").ok(),
                        });
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

pub async fn fetch_user(mut db: Connection<Db>, username: &str) -> Option<MarmakUser> {
    let query_result = sqlx::query(
        "SELECT password, perms, mirror_settings, email FROM users WHERE username = ? AND verified = 1",
    )
    .bind(username)
    .fetch_one(&mut **db)
    .await;

    match query_result {
        Ok(row) => {
            if let Some(perms) = row.try_get::<i32, _>("perms").ok() {
                let settings = row.try_get::<String, _>("mirror_settings").ok();
                return Some(MarmakUser {
                    username: username.to_string(),
                    password: row.try_get::<String, _>("password").ok().unwrap(),
                    perms: perms,
                    mirror_settings: settings,
                    email: row.try_get::<String, _>("email").ok(),
                });
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

pub async fn update_settings(mut db: Connection<Db>, username: &str, settings: &str) -> () {
    let _ = sqlx::query("UPDATE users SET mirror_settings = ? WHERE username = ?")
        .bind(settings)
        .bind(username)
        .fetch_one(&mut **db)
        .await;
}

pub async fn add_login(mut db: Connection<Db>, username: &str, ip: &str) -> () {
    let _ = sqlx::query("UPDATE users SET lastlogin_time = CURRENT_TIMESTAMP, lastlogin_ip = ?, lastlogin_via = 'MARMAK Mirror' WHERE username = ?")
    .bind(ip)
    .bind(username)
    .fetch_one(&mut **db)
    .await;
    let _ = sqlx::query("INSERT INTO logins (account, time, ip, via) VALUES (?, CURRENT_TIMESTAMP, ?, 'MARMAK Mirror')")
    .bind(username)
    .bind(ip)
    .fetch_one(&mut **db)
    .await;
}

pub async fn add_download(mut db: Connection<FileDb>, path: &str) -> () {
    let id = Uuid::new_v4().to_string();

    let _ = sqlx::query("INSERT INTO files (id, path, downloads) VALUES (?, ?, 1) ON DUPLICATE KEY UPDATE downloads = downloads + 1")
    .bind(id)
    .bind(path)
    .execute(&mut **db)
    .await;
}

pub async fn get_downloads(mut db: Connection<FileDb>, path: &str) -> Option<i32> {
    let query_result = sqlx::query("SELECT downloads FROM files WHERE path = ? OR id = ?")
        .bind(path)
        .bind(path)
        .fetch_one(&mut **db)
        .await;

    match query_result {
        Ok(row) => {
            if let Some(downloads) = row.try_get::<i32, _>("downloads").ok() {
                Some(downloads)
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

use rand::{distributions::Alphanumeric, Rng};
use rocket_db_pools::{sqlx, Connection, Database};
use sqlx::Row;

use bcrypt::verify;
use uuid::Uuid;

use crate::account::MarmakUser;

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
                let username = row
                    .try_get::<String, _>("username")
                    .ok()
                    .unwrap_or_default();
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
        Err(error) => {
            println!("Database error: {:?}", error);
            None
        }
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
                    password: row
                        .try_get::<String, _>("password")
                        .ok()
                        .unwrap_or_default(),
                    perms: perms,
                    mirror_settings: settings,
                    email: row.try_get::<String, _>("email").ok(),
                });
            } else {
                None
            }
        }
        Err(error) => {
            println!("Database error: {:?}", error);
            None
        }
    }
}

pub async fn update_settings(mut db: Connection<Db>, username: &str, settings: &str) -> () {
    if let Err(error) = sqlx::query("UPDATE users SET mirror_settings = ? WHERE username = ?")
        .bind(settings)
        .bind(username)
        .execute(&mut **db)
        .await
    {
        println!("Database error: {:?}", error);
    }
}

pub async fn add_login(mut db: Connection<Db>, username: &str, ip: &str) -> () {
    if let Err(error) = sqlx::query("INSERT INTO logins (account, time, ip, via) VALUES (?, CURRENT_TIMESTAMP, ?, 'MARMAK Mirror')")
        .bind(username)
        .bind(ip)
        .execute(&mut **db)
        .await
    {
        println!("Database error: {:?}", error);
    }
}

pub async fn add_download(mut db: Connection<FileDb>, path: &str) -> () {
    let id = Uuid::new_v4().to_string();

    if let Err(error) = sqlx::query("INSERT INTO files (id, path, downloads) VALUES (?, ?, 1) ON DUPLICATE KEY UPDATE downloads = downloads + 1")
        .bind(id)
        .bind(path)
        .execute(&mut **db)
        .await
    {
        println!("Database error: {:?}", error);
    }
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
        Err(error) => {
            println!("Database error: {:?}", error);
            None
        }
    }
}

pub async fn add_rememberme_token(mut db: Connection<Db>, username: &str) -> Option<String> {
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();

    if let Err(error) = sqlx::query("INSERT INTO sessions (id, user) VALUES (?, ?)")
        .bind(&token)
        .bind(username)
        .execute(&mut **db)
        .await
    {
        println!("Database error: {:?}", error);
        return None;
    }

    Some(token)
}

pub async fn delete_session(mut db: Connection<Db>, token: &str) -> () {
    if let Err(error) = sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(&token)
        .execute(&mut **db)
        .await
    {
        println!("Database error: {:?}", error);
    }
}

#[cfg(not(test))]
pub async fn fetch_user_by_session(mut db: Connection<Db>, id: &str) -> Option<MarmakUser> {
    let query_result = sqlx::query("SELECT user FROM sessions WHERE id = ?")
        .bind(id)
        .fetch_one(&mut **db)
        .await;

    match query_result {
        Ok(row) => {
            if let Some(user) = row.try_get::<String, _>("user").ok() {
                fetch_user(db, &user).await
            } else {
                None
            }
        }
        Err(error) => {
            println!("Database error: {:?}", error);
            None
        }
    }
}

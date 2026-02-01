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
) -> Option<MarmakUser> {
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

                return Some(MarmakUser {
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

pub async fn get_user(mut db: Connection<Db>, username: &str) -> Option<MarmakUser> {
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

pub async fn update_settings(mut db: Connection<Db>, username: &str, settings: &str) -> () {
    if let Err(error) = sqlx::query("UPDATE users SET mirror_settings = ? WHERE username = ?")
        .bind(settings)
        .bind(username)
        .execute(&mut **db)
        .await
    {
        eprintln!("Database error (update_settings): {:?}", error);
    }
}

pub async fn add_login(mut db: Connection<Db>, username: &str, ip: &str) -> () {
    if let Err(error) = sqlx::query("INSERT INTO logins (account, time, ip, via) VALUES (?, CURRENT_TIMESTAMP, ?, 'MARMAK Mirror')")
        .bind(username)
        .bind(ip)
        .execute(&mut **db)
        .await
    {
        eprintln!("Database error (add_login): {:?}", error);
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
        eprintln!("Database error (add_download): {:?}", error);
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
            eprintln!("Database error (get_downloads): {:?}", error);
            None
        }
    }
}

pub async fn get_file_by_id(mut db: Connection<FileDb>, path: &str) -> Option<String> {
    let query_result = sqlx::query("SELECT path FROM files WHERE path = ? OR id = ?")
        .bind(path)
        .bind(path)
        .fetch_one(&mut **db)
        .await;

    match query_result {
        Ok(row) => {
            if let Some(path) = row.try_get::<String, _>("path").ok() {
                Some(path)
            } else {
                None
            }
        }
        Err(error) => {
            eprintln!("Database error (get_file_by_id): {:?}", error);
            None
        }
    }
}

pub async fn add_shared_file(mut db: Connection<FileDb>, path: &str) -> Option<String> {
    let id = Uuid::new_v4().to_string();

    if let Ok(result) = sqlx::query("SELECT id FROM files WHERE path = ?")
        .bind(path)
        .fetch_one(&mut **db)
        .await
    {
        return result.try_get::<String, _>("id").ok();
    }

    if let Err(error) = sqlx::query("INSERT INTO files (id, path, downloads, shared) VALUES (?, ?, 0, 1) ON DUPLICATE KEY UPDATE downloads = downloads + 1")
        .bind(&id)
        .bind(path)
        .execute(&mut **db)
        .await
    {
        eprintln!("Database error (add_shared_file): {:?}", error);
        None
    } else {
        Some(id)
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
        eprintln!("Database error (add_rememberme_token): {:?}", error);
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
        eprintln!("Database error (delete_session): {:?}", error);
    }
}

#[cfg(not(test))]
pub async fn get_user_by_session(mut db: Connection<Db>, id: &str) -> Option<MarmakUser> {
    let query_result = sqlx::query("SELECT user FROM sessions WHERE id = ?")
        .bind(id)
        .fetch_one(&mut **db)
        .await;

    match query_result {
        Ok(row) => {
            if let Some(user) = row.try_get::<String, _>("user").ok() {
                get_user(db, &user).await
            } else {
                None
            }
        }
        Err(error) => {
            eprintln!("Database error (get_user_by_session): {:?}", error);
            None
        }
    }
}

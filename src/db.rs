use rand::{distributions::Alphanumeric, Rng};
use rocket_db_pools::{sqlx, Connection, Database};
use sqlx::Row;

use uuid::Uuid;

#[cfg(not(test))]
use crate::account::MarmakUser;

#[derive(Database)]
#[database("marmak")]
pub struct Db(sqlx::MySqlPool);

#[derive(Database)]
#[database("mirror")]
pub struct FileDb(sqlx::MySqlPool);

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

pub async fn delete_file(mut db: Connection<FileDb>, path: &str) -> () {
    if let Err(error) = sqlx::query("DELETE FROM files WHERE path = ?")
        .bind(path)
        .execute(&mut **db)
        .await
    {
        eprintln!("Database error (delete_file): {:?}", error);
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
                MarmakUser::get(db, &user).await
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

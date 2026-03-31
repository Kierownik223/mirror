use std::{cmp::Ordering, collections::HashMap, ffi::OsStr, fs, path::{Path, PathBuf}};

use once_cell::sync::Lazy;
use rocket::{fs::NamedFile, http::Status};
use rocket_db_pools::{Connection, sqlx::{self, Row}};
use uuid::Uuid;

use crate::{config::CONFIG, db::FileDb, guards::HeaderFile, responders::{IndexResponse, IndexResult}};

static SHARED_ICONS: Lazy<HashMap<String, String>> = Lazy::new(crate::load_shared_icons);

#[derive(PartialOrd)]
pub struct MirrorFileInternal {
    pub mirror_file: MirrorFile,
    pub id: Option<String>,
    pub path: String,
}

impl Eq for MirrorFileInternal {}

impl PartialEq for MirrorFileInternal {
    fn eq(&self, other: &Self) -> bool {
        (&self.mirror_file.name, &self.mirror_file.ext)
            == (&other.mirror_file.name, &other.mirror_file.ext)
    }
}

impl Ord for MirrorFileInternal {
    fn cmp(&self, other: &Self) -> Ordering {
        self.mirror_file.name.cmp(&other.mirror_file.name)
    }
}

impl Default for MirrorFileInternal {
    fn default() -> Self {
        MirrorFileInternal {
            mirror_file: MirrorFile {
                name: "".into(),
                ext: "".into(),
                icon: "default".into(),
                size: 0,
                downloads: None,
            },
            id: None,
            path: "files/".into(),
        }
    }
}

impl MirrorFileInternal {
    pub async fn load(mut db: Connection<FileDb>, path: &PathBuf) -> Option<Self> {
        let md = fs::metadata(&path).ok()?;
        let name = MirrorFile::get_name_from_path(&path);
        let ext = if md.is_file() {
            MirrorFile::get_extension_from_path(&path)
        } else {
            "folder".into()
        };
        let icon = MirrorFile::get_icon(&MirrorFile::get_name_from_path(&path));

        let query_result = sqlx::query("SELECT id, downloads FROM files WHERE path = ?")
            .bind(path.display().to_string().replacen("files/", "", 1))
            .fetch_one(&mut **db)
            .await;

        let (id, downloads) = match query_result {
            Ok(row) => (
                row.try_get::<String, _>("id").ok(),
                row.try_get::<i32, _>("downloads").ok(),
            ),
            Err(error) => {
                eprintln!("Database error (get_file_by_id): {:?}", error);
                (None, None)
            }
        };

        Some(MirrorFileInternal {
            mirror_file: MirrorFile {
                name,
                ext,
                icon,
                size: md.len(),
                downloads,
            },
            id,
            path: path.display().to_string().replacen("files/", "/", 1),
        })
    }

    pub async fn load_and_share(mut db: Connection<FileDb>, path: &PathBuf) -> Option<Self> {
        let md = fs::metadata(&path).ok()?;
        let name = MirrorFile::get_name_from_path(&path);
        let ext = if md.is_file() {
            MirrorFile::get_extension_from_path(&path)
        } else {
            "folder".into()
        };
        let icon = MirrorFile::get_icon(&MirrorFile::get_name_from_path(&path));

        let query_result = sqlx::query("SELECT id, downloads FROM files WHERE path = ?")
            .bind(path.display().to_string().replacen("files/", "", 1))
            .fetch_one(&mut **db)
            .await;

        let (id, downloads) = match query_result {
            Ok(row) => (
                row.try_get::<String, _>("id").ok().unwrap_or(Uuid::new_v4().to_string()),
                row.try_get::<i32, _>("downloads").ok(),
            ),
            Err(error) => {
                eprintln!("Database error (MirrorFile::load_and_share [get_file_by_id]): {:?}", error);
                (Uuid::new_v4().to_string(), None)
            }
        };

        if let Err(error) = sqlx::query("INSERT INTO files (id, path, downloads, shared) VALUES (?, ?, 0, 1) ON DUPLICATE KEY UPDATE downloads = downloads + 1")
            .bind(&id)
            .bind(&path.display().to_string().trim_start_matches("files/"))
            .execute(&mut **db)
            .await
        {
            eprintln!("Database error (MirrorFile::load_and_share [share]): {:?}", error);
        }

        Some(MirrorFileInternal {
            mirror_file: MirrorFile {
                name,
                ext,
                icon,
                size: md.len(),
                downloads,
            },
            id: Some(id),
            path: path.display().to_string().replacen("files/", "/", 1),
        })
    }

    pub async fn load_by_id(mut db: Connection<FileDb>, id: &str) -> Option<Self> {
        let query_result = sqlx::query("SELECT path, downloads FROM files WHERE id = ?")
            .bind(id)
            .fetch_one(&mut **db)
            .await;

        let (path, downloads) = match query_result {
            Ok(row) => Some((
                row.try_get::<String, _>("path")
                    .ok()
                    .map(|f| format!("files/{}", f))?,
                row.try_get::<i32, _>("downloads").ok(),
            )),
            Err(error) => {
                eprintln!("Database error (get_file_by_id): {:?}", error);
                None
            }
        }?;

        let file_path = Path::new(&path).to_path_buf();

        let md = fs::metadata(&file_path).ok()?;
        let name = MirrorFile::get_name_from_path(&file_path);
        let ext = if md.is_file() {
            MirrorFile::get_extension_from_path(&file_path)
        } else {
            "folder".into()
        };
        let icon = MirrorFile::get_icon(&MirrorFile::get_name_from_path(&file_path));

        Some(MirrorFileInternal {
            mirror_file: MirrorFile {
                name,
                ext,
                icon,
                size: md.len(),
                downloads,
            },
            id: Some(id.to_string()),
            path: file_path.display().to_string().replacen("files/", "/", 1),
        })
    }

    pub async fn add_download(&self, mut db: Connection<FileDb>) -> () {
        if let Err(error) = sqlx::query("INSERT INTO files (id, path, downloads) VALUES (?, ?, 1) ON DUPLICATE KEY UPDATE downloads = downloads + 1")
            .bind(&self.id)
            .bind(&self.path)
            .execute(&mut **db)
            .await
        {
            eprintln!("Database error (add_download): {:?}", error);
        }
    }

    pub async fn open_file(path: PathBuf, cache_control: &str) -> IndexResult {
        if !path.exists() {
            return Err(Status::NotFound);
        }

        if CONFIG.standalone {
            match NamedFile::open(&path).await {
                Ok(f) => Ok(IndexResponse::NamedFile(f, cache_control.to_string())),
                Err(_) => Err(Status::InternalServerError),
            }
        } else {
            Ok(IndexResponse::HeaderFile(HeaderFile(
                path.display().to_string(),
                cache_control.to_string(),
            )))
        }
    }

    #[allow(unused)] // Reserved for future use
    async fn share(&mut self, mut db: Connection<FileDb>) -> bool {
        self.id = if let Ok(result) = sqlx::query("SELECT id FROM files WHERE path = ?")
            .bind(&self.path)
            .fetch_one(&mut **db)
            .await
        {
            result.try_get::<String, _>("id").ok()
        } else {
            Some(Uuid::new_v4().to_string())
        };

        if let Err(error) = sqlx::query("INSERT INTO files (id, path, downloads, shared) VALUES (?, ?, 0, 1) ON DUPLICATE KEY UPDATE downloads = downloads + 1")
            .bind(&self.id)
            .bind(&self.path)
            .execute(&mut **db)
            .await
        {
            eprintln!("Database error (add_shared_file): {:?}", error);
            false
        } else {
            true
        }
    }
}

#[derive(serde::Serialize, PartialOrd, serde::Deserialize)]
pub struct MirrorFile {
    pub name: String,
    pub ext: String,
    pub icon: String,
    pub size: u64,
    pub downloads: Option<i32>,
}

impl Eq for MirrorFile {}

impl PartialEq for MirrorFile {
    fn eq(&self, other: &Self) -> bool {
        (&self.name, &self.ext) == (&other.name, &other.ext)
    }
}

impl Ord for MirrorFile {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

impl Default for MirrorFile {
    fn default() -> Self {
        MirrorFile {
            name: "".into(),
            ext: "".into(),
            icon: "default".into(),
            size: 0,
            downloads: None,
        }
    }
}

impl MirrorFile {
    pub fn is_dir(&self) -> bool {
        if self.ext == "folder" || self.ext == "privatefolder" {
            true
        } else {
            false
        }
    }

    pub fn load(path: &PathBuf) -> Option<Self> {
        let md = fs::metadata(&path).ok()?;
        let name = MirrorFile::get_name_from_path(&path);
        let ext = if md.is_file() {
            MirrorFile::get_extension_from_path(&path)
        } else {
            "folder".into()
        };
        let icon = MirrorFile::get_icon(&MirrorFile::get_name_from_path(&path));

        Some(MirrorFile {
            name,
            ext,
            icon,
            size: md.len(),
            downloads: None,
        })
    }

    pub fn new(file_name: &str) -> Self {
        let ext = MirrorFile::get_extension_from_filename(file_name).unwrap_or_default();
        let icon = MirrorFile::get_icon(file_name);

        MirrorFile {
            name: file_name.into(),
            ext: ext.into(),
            icon,
            size: 0,
            downloads: None,
        }
    }

    pub fn new_folder(file_name: &str) -> Self {
        MirrorFile {
            name: file_name.into(),
            ext: "folder".into(),
            icon: "folder".into(),
            size: 0,
            downloads: None,
        }
    }

    fn get_shared_icon<'a>(ext: &'a str) -> &'a str {
        SHARED_ICONS.get(ext).map(|s| s.as_str()).unwrap_or(ext)
    }

    pub fn get_extension_from_filename(filename: &str) -> Option<&str> {
        Path::new(filename).extension().and_then(OsStr::to_str)
    }

    pub fn get_icon(file_name: &str) -> String {
        let ext = Self::get_extension_from_filename(file_name)
            .unwrap_or_else(|| "")
            .to_lowercase();

        let mut icon = Self::get_shared_icon(&ext);

        if !Path::new(&format!("public/static/images/icons/{}.png", &icon)).exists() {
            icon = "default";
        }

        icon.to_string()
    }

    pub fn get_cache_control(is_private: bool) -> String {
        if is_private {
            "private".into()
        } else if CONFIG.max_age == 0 {
            "public".into()
        } else {
            format!("public, max-age={}", CONFIG.max_age)
        }
    }

    pub fn get_static_cache_control() -> String {
        if CONFIG.static_max_age == 0 {
            "public".into()
        } else {
            format!("public, max-age={}", CONFIG.static_max_age)
        }
    }

    pub fn get_name_from_path(path: &PathBuf) -> String {
        path.file_name()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default()
            .to_string()
    }

    pub fn get_extension_from_path(path: &PathBuf) -> String {
        path.extension()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default()
            .to_string()
    }

    pub fn get_virtual_path(path: &str) -> String {
        Path::new(&path.replace("files/", "/"))
            .display()
            .to_string()
    }

    pub fn is_restricted(path: &Path, is_logged_in: bool) -> bool {
        if !CONFIG.enable_login {
            return false;
        }
        if path.ends_with("cover.png")
            || path.ends_with("cover.jpg")
            || path.ends_with("folder.png")
            || path.ends_with("folder.jpg")
        {
            return false;
        }
        let mut current = Some(path);

        while let Some(p) = current {
            if p.join("RESTRICTED").exists() {
                return !is_logged_in;
            }
            current = p.parent();
        }

        false
    }

    pub fn is_hidden(path: &Path, perms: Option<i32>) -> bool {
        let mut current = Some(path);

        while let Some(p) = current {
            if p.join("HIDDEN").exists() {
                if let Some(perms) = perms {
                    return perms != 0;
                } else {
                    return true;
                }
            }
            current = p.parent();
        }

        false
    }

    pub fn get_real_path(file: &PathBuf, username: String) -> Result<(PathBuf, bool), Status> {
        if let Ok(rest) = file.strip_prefix("private") {
            if username == "Nobody" {
                return Err(Status::Forbidden);
            }

            Ok((
                Path::new("files/")
                    .join("private")
                    .join(&username)
                    .join(rest),
                true,
            ))
        } else {
            Ok((Path::new("files/").join(&file), false))
        }
    }

    pub fn get_real_path_with_perms(
        file: &PathBuf,
        username: String,
        perms: i32,
    ) -> Result<(PathBuf, bool), Status> {
        if let Ok(rest) = file.strip_prefix("private") {
            if username == "Nobody" {
                return Err(Status::Forbidden);
            }

            Ok((
                Path::new("files/")
                    .join("private")
                    .join(&username)
                    .join(rest),
                true,
            ))
        } else {
            if perms != 0 {
                Err(Status::Forbidden)
            } else {
                Ok((Path::new("files/").join(&file), false))
            }
        }
    }
}

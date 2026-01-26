use rocket::{
    fs::NamedFile,
    http::{ContentType, Status},
    response::{self, Redirect, Responder},
    serde::json::Json,
    Request,
};
use rocket_dyn_templates::Template;

use crate::{
    MirrorFile, Sysinfo, api::{ApiInfoResponse, MusicFile, SearchFile, UploadFile, UploadLimits, VideoFile}, guards::HeaderFile
};

pub struct Cached<R> {
    pub response: R,
    pub header: &'static str,
}

impl<'r, R: 'r + Responder<'r, 'static> + Send> Responder<'r, 'static> for Cached<R> {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'static> {
        let mut res = self.response.respond_to(request)?;

        res.set_raw_header("Cache-Control", self.header);

        Ok(res)
    }
}

pub enum IndexResponse {
    Template(Template),
    DirectFile((ContentType, Vec<u8>), String),
    HeaderFile(HeaderFile),
    NamedFile(NamedFile, String),
    Redirect(Redirect),
}

pub type IndexResult = Result<IndexResponse, Status>;

#[rocket::async_trait]
impl<'r> Responder<'r, 'r> for IndexResponse {
    fn respond_to(self, req: &'r rocket::Request<'_>) -> rocket::response::Result<'r> {
        match self {
            IndexResponse::Template(t) => {
                let mut res = t.respond_to(req)?;
                res.set_raw_header("Cache-Control", "private");
                Ok(res)
            }
            IndexResponse::DirectFile(d, cache_control) => {
                let mut res = d.respond_to(req)?;
                res.set_raw_header("Cache-Control", cache_control);
                Ok(res)
            }
            IndexResponse::HeaderFile(h) => h.respond_to(req),
            IndexResponse::NamedFile(f, cache_control) => {
                let mut res = f.respond_to(req)?;
                res.set_raw_header("Cache-Control", cache_control);
                Ok(res)
            }
            IndexResponse::Redirect(r) => r.respond_to(req),
        }
    }
}

pub enum ApiResponse {
    Files(Json<Vec<MirrorFile>>),
    File(Json<MirrorFile>),
    MusicFile(Json<MusicFile>),
    VideoFile(Json<VideoFile>),
    MessageStatus((Status, Json<ApiInfoResponse>)),
    Message(Json<ApiInfoResponse>),
    Sysinfo(Json<Sysinfo>),
    UploadFiles(Json<Vec<UploadFile>>),
    SearchResults(Json<Vec<SearchFile>>),
    UploadLimits(Json<UploadLimits>),
}

pub type ApiResult = Result<ApiResponse, Status>;

#[rocket::async_trait]
impl<'r> Responder<'r, 'r> for ApiResponse {
    fn respond_to(self, req: &'r rocket::Request<'_>) -> rocket::response::Result<'r> {
        match self {
            ApiResponse::Files(f) => {
                let mut res = f.respond_to(req)?;
                res.set_raw_header("Cache-Control", "no-cache");
                Ok(res)
            }
            ApiResponse::File(f) => {
                let mut res = f.respond_to(req)?;
                res.set_raw_header("Cache-Control", "no-cache");
                Ok(res)
            }
            ApiResponse::MusicFile(f) => {
                let mut res = f.respond_to(req)?;
                res.set_raw_header("Cache-Control", "private");
                Ok(res)
            }
            ApiResponse::VideoFile(f) => {
                let mut res = f.respond_to(req)?;
                res.set_raw_header("Cache-Control", "private");
                Ok(res)
            }
            ApiResponse::MessageStatus(m) => {
                let mut res = m.respond_to(req)?;
                res.set_raw_header("Cache-Control", "no-cache");
                Ok(res)
            }
            ApiResponse::Message(m) => {
                let mut res = m.respond_to(req)?;
                res.set_raw_header("Cache-Control", "no-cache");
                Ok(res)
            }
            ApiResponse::Sysinfo(s) => {
                let mut res = s.respond_to(req)?;
                res.set_raw_header("Cache-Control", "no-cache");
                Ok(res)
            }
            ApiResponse::UploadFiles(f) => {
                let mut res = f.respond_to(req)?;
                res.set_raw_header("Cache-Control", "no-cache");
                Ok(res)
            }
            ApiResponse::SearchResults(s) => {
                let mut res = s.respond_to(req)?;
                res.set_raw_header("Cache-Control", "no-cache");
                Ok(res)
            }
            ApiResponse::UploadLimits(l) => {
                let mut res = l.respond_to(req)?;
                res.set_raw_header("Cache-Control", "no-cache");
                Ok(res)
            }
        }
    }
}

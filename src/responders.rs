use rocket::{
    fs::NamedFile,
    http::Status,
    response::{self, Redirect, Responder},
    Request,
};
use rocket_dyn_templates::Template;

use crate::guards::HeaderFile;

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

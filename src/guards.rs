use rocket::{
    request::{FromRequest, Outcome},
    response::{self, Responder},
    Request, Response,
};

use crate::config::CONFIG;
pub struct HeaderFile(pub String, pub String);

impl<'r> Responder<'r, 'r> for HeaderFile {
    fn respond_to(self, _: &Request<'_>) -> response::Result<'r> {
        let mut builder = Response::build();

        builder.raw_header(
            &CONFIG.x_sendfile_header,
            format!(
                "{}{}",
                CONFIG.x_sendfile_prefix,
                urlencoding::encode(&self.0).replace("%2F", "/")
            ),
        );

        builder.raw_header("Cache-Control", self.1);

        builder.ok()
    }
}

pub struct XForwardedFor<'r>(pub &'r str);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for XForwardedFor<'r> {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match request.headers().get_one("X-Forwarded-For") {
            Some(value) => {
                let mut ip = value.split(',').next().map(str::trim).unwrap_or(value);

                if ip == "127.0.0.1" || ip == "::1" {
                    ip = "(unknown)";
                }

                Outcome::Success(XForwardedFor(ip))
            }
            None => Outcome::Success(XForwardedFor("(unknown)")),
        }
    }
}

pub struct Host<'r>(pub &'r str);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Host<'r> {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match request.headers().get_one("Host") {
            Some(value) => Outcome::Success(Host(value)),
            None => Outcome::Success(Host("127.0.0.1")),
        }
    }
}

pub struct FullUri(pub String);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for FullUri {
    type Error = ();

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let uri = req.uri().to_string();
        Outcome::Success(FullUri(uri))
    }
}

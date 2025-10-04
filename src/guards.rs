use rocket::{
    request::{FromRequest, Outcome},
    response::{self, Responder},
    Request, Response,
};

use crate::{config::CONFIG, utils::get_bool_cookie};

#[derive(FromForm, serde::Serialize)]
pub struct Settings<'r> {
    pub theme: Option<&'r str>,
    pub lang: Option<&'r str>,
    pub hires: Option<&'r str>,
    pub smallhead: Option<&'r str>,
    pub plain: Option<&'r str>,
    pub nooverride: Option<&'r str>,
    pub viewers: Option<&'r str>,
    pub filebrowser: Option<&'r str>,
}

pub struct HeaderFile(pub String, pub String);

impl<'r> Responder<'r, 'r> for HeaderFile {
    fn respond_to(self, _: &Request<'_>) -> response::Result<'r> {
        let mut builder = Response::build();

        builder.raw_header(
            &CONFIG.x_sendfile_header,
            format!("{}{}", CONFIG.x_sendfile_prefix, self.0),
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
pub struct UsePlain<'r>(pub &'r bool);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for UsePlain<'r> {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match request.headers().get_one("User-Agent") {
            Some(value) => {
                if get_bool_cookie(request.cookies(), "plain", false) {
                    return Outcome::Success(UsePlain(&true));
                }

                if value.starts_with("Mozilla/1") || value.starts_with("Mozilla/2") {
                    return Outcome::Success(UsePlain(&true));
                }

                Outcome::Success(UsePlain(&false))
            }
            None => Outcome::Success(UsePlain(&true)),
        }
    }
}

pub struct UseViewers<'r>(pub &'r bool);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for UseViewers<'r> {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match request.headers().get_one("User-Agent") {
            Some(value) => {
                if value.starts_with("Winamp") || value.starts_with("VLC") {
                    return Outcome::Success(UseViewers(&false));
                }

                if get_bool_cookie(request.cookies(), "viewers", true) {
                    return Outcome::Success(UseViewers(&true));
                }

                Outcome::Success(UseViewers(&false))
            }
            None => Outcome::Success(UseViewers(&true)),
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

use rocket::{
    http::CookieJar,
    request::{FromRequest, Outcome},
    response::{self, Responder},
    Request, Response,
};

use crate::config::CONFIG;

#[derive(FromForm, serde::Serialize)]
pub struct FormSettings<'r> {
    pub theme: Option<&'r str>,
    pub lang: Option<&'r str>,
    pub hires: Option<&'r str>,
    pub smallhead: Option<&'r str>,
    pub plain: Option<&'r str>,
    pub nooverride: Option<&'r str>,
    pub viewers: Option<&'r str>,
    pub dir_browser: Option<&'r str>,
    pub use_si: Option<&'r str>,
    pub audio_player: Option<&'r str>,
    pub video_player: Option<&'r str>,
    pub show_cover: Option<&'r str>,
}

#[derive(serde::Serialize)]
pub struct Settings<'r> {
    pub theme: &'r str,
    pub js_present: bool,
    pub lang: &'r str,
    pub hires: bool,
    pub smallhead: bool,
    pub plain: bool,
    pub nooverride: bool,
    pub viewers: bool,
    pub dir_browser: bool,
    pub use_si: bool,
    pub audio_player: bool,
    pub video_player: bool,
    pub show_cover: bool,
}

impl<'r> Settings<'r> {
    pub fn from_cookies(jar: &'r CookieJar<'_>) -> Self {
        let mut theme = jar
            .get("theme")
            .map(|cookie| cookie.value())
            .unwrap_or("default");

        if !std::path::Path::new(&format!("public/static/styles/{}.css", &theme)).exists() {
            theme = "default";
        }

        let lang = if let Some(cookie_lang) = jar.get("lang").map(|c| c.value()) {
            cookie_lang
        } else {
            "en"
        };

        let hires = jar
            .get("hires")
            .map(|c| c.value() == "true")
            .unwrap_or(false);

        let smallhead = jar
            .get("smallhead")
            .map(|c| c.value() == "true")
            .unwrap_or(false);

        let plain = jar
            .get("plain")
            .map(|c| c.value() == "true")
            .unwrap_or(false);

        let nooverride = jar
            .get("nooverride")
            .map(|c| c.value() == "true")
            .unwrap_or(false);

        let viewers = jar
            .get("viewers")
            .map(|c| c.value() == "true")
            .unwrap_or(true);

        let dir_browser = jar
            .get("dir_browser")
            .map(|c| c.value() == "true")
            .unwrap_or(true);

        let use_si = jar
            .get("use_si")
            .map(|c| c.value() == "true")
            .unwrap_or(true);

        let audio_player = jar
            .get("audio_player")
            .map(|c| c.value() == "true")
            .unwrap_or(true);

        let video_player = jar
            .get("video_player")
            .map(|c| c.value() == "true")
            .unwrap_or(true);

        let show_cover: bool = jar
            .get("show_cover")
            .map(|c| c.value() == "true")
            .unwrap_or(true);

        Self {
            theme,
            js_present: std::path::Path::new(&format!("public/static/styles/{}.js", &theme))
                .exists(),
            lang: lang,
            hires,
            smallhead,
            plain,
            nooverride,
            viewers,
            dir_browser,
            use_si,
            audio_player,
            video_player,
            show_cover,
        }
    }
}

impl Default for Settings<'_> {
    fn default() -> Self {
        Settings {
            theme: "default",
            js_present: false,
            lang: "en",
            hires: false,
            smallhead: false,
            plain: false,
            nooverride: false,
            viewers: true,
            dir_browser: true,
            use_si: true,
            audio_player: true,
            video_player: true,
            show_cover: true,
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Settings<'r> {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let mut settings = Settings::from_cookies(request.cookies());

        settings.plain = match (
            request.cookies().get("plain").map(|c| c.value() == "true"),
            request.headers().get_one("User-Agent"),
        ) {
            (Some(value), _) => value,

            (None, Some(ua)) => {
                ua.starts_with("Mozilla/1")
                    || ua.starts_with("Mozilla/2")
                    || ua.starts_with("Links")
                    || ua.starts_with("Lynx")
            }

            (None, None) => true,
        };

        settings.viewers = match (
            request
                .cookies()
                .get("viewers")
                .map(|c| c.value() == "true"),
            request.headers().get_one("User-Agent"),
        ) {
            (Some(value), _) => value,

            (None, Some(ua)) => {
                !(ua.starts_with("Mozilla/1")
                    || ua.starts_with("Mozilla/2")
                    || ua.starts_with("Links")
                    || ua.starts_with("Lynx")
                    || ua.starts_with("Winamp")
                    || ua.starts_with("VLC")
                    || ua.starts_with("curl"))
            }

            (None, None) => false,
        };
        settings.audio_player = match (
            request
                .cookies()
                .get("audio_player")
                .map(|c| c.value() == "true"),
            request.headers().get_one("User-Agent"),
        ) {
            (Some(value), _) => value,

            (None, Some(ua)) => {
                !(ua.starts_with("Mozilla/1")
                    || ua.starts_with("Mozilla/2")
                    || ua.starts_with("Links")
                    || ua.starts_with("Lynx")
                    || ua.starts_with("Winamp")
                    || ua.starts_with("VLC")
                    || ua.starts_with("curl"))
            }

            (None, None) => false,
        };
        settings.video_player = match (
            request
                .cookies()
                .get("video_player")
                .map(|c| c.value() == "true"),
            request.headers().get_one("User-Agent"),
        ) {
            (Some(value), _) => value,

            (None, Some(ua)) => {
                !(ua.starts_with("Mozilla/1")
                    || ua.starts_with("Mozilla/2")
                    || ua.starts_with("Links")
                    || ua.starts_with("Lynx")
                    || ua.starts_with("Winamp")
                    || ua.starts_with("VLC")
                    || ua.starts_with("curl"))
            }

            (None, None) => false,
        };

        rocket::outcome::Outcome::Success(settings)
    }
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

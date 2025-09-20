use chrono::Utc;
use jsonwebtoken::{
    decode, encode,
    errors::{Error, ErrorKind},
    Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};
use serde::{Deserialize, Serialize};

use crate::{config::Config, MarmakUser};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Claims {
    pub username: String,
    pub email: Option<String>,
    pub perms: i32,
    exp: usize,
    iat: usize,
}

#[derive(Debug, Clone)]
pub struct JWT {
    pub claims: Claims,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for JWT {
    type Error = Status;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Status> {
        fn is_valid(key: &str) -> Result<Claims, Error> {
            Ok(decode_jwt(String::from(key))?)
        }

        let authorization = match req.headers().get_one("authorization") {
            Some(token) => Some(token),
            None => match req.cookies().get("matoken") {
                Some(cookie) => Some(cookie.value()),
                None => None,
            },
        };

        match authorization {
            None => Outcome::Error((Status::Unauthorized, Status::Unauthorized)),
            Some(key) => match is_valid(key) {
                Ok(claims) => Outcome::Success(JWT { claims }),
                Err(_) => Outcome::Error((Status::Unauthorized, Status::Unauthorized)),
            },
        }
    }
}

impl Default for JWT {
    fn default() -> Self {
        JWT { claims: Claims { username: "Nobody".into(), email: None, perms: 1, exp: 1, iat: 0 } }
    }
}

pub fn create_jwt(user: &MarmakUser) -> Result<String, Error> {
    let secret = Config::load().jwt_secret;

    let expiration = Utc::now()
        .checked_add_signed(chrono::Duration::seconds(3600))
        .expect("Invalid timestamp")
        .timestamp();

    let claims = Claims {
        username: (*user.username).to_string(),
        email: user.email.clone(),
        perms: user.perms,
        exp: expiration as usize,
        iat: Utc::now().timestamp() as usize,
    };

    let header = Header::new(Algorithm::HS512);

    encode(
        &header,
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn decode_jwt(token: String) -> Result<Claims, ErrorKind> {
    let secret = Config::load().jwt_secret;

    let token = token.trim_start_matches("Bearer").trim();

    match decode::<Claims>(
        &token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS512),
    ) {
        Ok(token) => Ok(token.claims),
        Err(err) => Err(err.kind().to_owned()),
    }
}

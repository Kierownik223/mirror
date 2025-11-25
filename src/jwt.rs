use chrono::Utc;
use jsonwebtoken::{encode, errors::Error, Algorithm, EncodingKey, Header};

#[cfg(not(test))]
use jsonwebtoken::{decode, errors::ErrorKind, DecodingKey, Validation};
use rocket::{
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};
#[cfg(not(test))]
use rocket_db_pools::Connection;
use serde::{Deserialize, Serialize};

use crate::{account::MarmakUser, config::CONFIG};

#[cfg(not(test))]
use crate::db::{fetch_user_by_session, Db};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub email: Option<String>,
    pub perms: i32,
    exp: usize,
    iat: usize,
}

#[derive(Debug, Clone)]
pub struct JWT {
    pub claims: Claims,
    pub token: Option<String>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for JWT {
    type Error = Status;

    #[cfg(not(test))]
    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Status> {
        fn is_valid(key: &str) -> Result<Claims, Error> {
            Ok(decode_jwt(key)?)
        }

        let result = match req.headers().get_one("authorization") {
            Some(token) => Some((token.to_string(), false)),
            None => match req.cookies().get("matoken") {
                Some(cookie) => Some((cookie.value().to_string(), false)),
                None => match req.cookies().get("token") {
                    Some(cookie) => Some((cookie.value().to_string(), false)),
                    None => match req.cookies().get("maremembermetoken") {
                        Some(rememberme_cookie) => {
                            let db = req.guard::<Connection<Db>>().await.unwrap();
                            if let Some(user) =
                                fetch_user_by_session(db, rememberme_cookie.value()).await
                            {
                                Some((create_jwt(&user).unwrap(), true))
                            } else {
                                None
                            }
                        }
                        None => match req.cookies().get("remembermetoken") {
                            Some(rememberme_cookie) => {
                                let db = req.guard::<Connection<Db>>().await.unwrap();
                                if let Some(user) =
                                    fetch_user_by_session(db, rememberme_cookie.value()).await
                                {
                                    Some((create_jwt(&user).unwrap(), true))
                                } else {
                                    None
                                }
                            }
                            None => None,
                        },
                    },
                },
            },
        };

        match result {
            None => Outcome::Error((Status::Unauthorized, Status::Unauthorized)),
            Some((key, readd_token)) => match is_valid(&key) {
                Ok(claims) => Outcome::Success(JWT {
                    claims,
                    token: if readd_token { Some(key) } else { None },
                }),
                Err(_) => match req.cookies().get("maremembermetoken") {
                    Some(rememberme_cookie) => {
                        let db = req.guard::<Connection<Db>>().await.unwrap();
                        if let Some(user) =
                            fetch_user_by_session(db, rememberme_cookie.value()).await
                        {
                            let token = create_jwt(&user).unwrap();
                            let claims = decode_jwt(&token).unwrap();

                            Outcome::Success(JWT {
                                claims,
                                token: Some(token),
                            })
                        } else {
                            Outcome::Error((Status::Unauthorized, Status::Unauthorized))
                        }
                    }
                    None => match req.cookies().get("remembermetoken") {
                        Some(rememberme_cookie) => {
                            let db = req.guard::<Connection<Db>>().await.unwrap();
                            if let Some(user) =
                                fetch_user_by_session(db, rememberme_cookie.value()).await
                            {
                                let token = create_jwt(&user).unwrap();
                                let claims = decode_jwt(&token).unwrap();

                                Outcome::Success(JWT {
                                    claims,
                                    token: Some(token),
                                })
                            } else {
                                Outcome::Error((Status::Unauthorized, Status::Unauthorized))
                            }
                        }
                        None => Outcome::Error((Status::Unauthorized, Status::Unauthorized)),
                    },
                },
            },
        }
    }

    #[cfg(test)]
    async fn from_request(_req: &'r Request<'_>) -> Outcome<Self, Status> {
        Outcome::Success(JWT {
            claims: Claims {
                sub: "test".into(),
                email: None,
                perms: 0,
                exp: 1,
                iat: 0,
            },
            token: None,
        })
    }
}

impl Default for JWT {
    fn default() -> Self {
        JWT {
            claims: Claims {
                sub: "Nobody".into(),
                email: None,
                perms: 1,
                exp: 1,
                iat: 0,
            },
            token: None,
        }
    }
}

pub fn create_jwt(user: &MarmakUser) -> Result<String, Error> {
    let secret = &CONFIG.jwt_secret;

    let expiration = Utc::now()
        .checked_add_signed(chrono::Duration::seconds(3600))
        .expect("Invalid timestamp")
        .timestamp();

    let claims = Claims {
        sub: (*user.username).to_string(),
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

#[cfg(not(test))]
pub fn decode_jwt(token: &str) -> Result<Claims, ErrorKind> {
    let secret = &CONFIG.jwt_secret;

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

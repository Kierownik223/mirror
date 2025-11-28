use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        fn validate_token(token: &str) -> Result<Claims, ErrorKind> {
            decode_jwt(token)
        }

        async fn refresh_with_code<'r>(
            req: &'r Request<'_>,
            code: &str,
        ) -> Option<(String, Claims)> {
            let db = req.guard::<Connection<Db>>().await.succeeded()?;

            let user = fetch_user_by_session(db, code).await?;
            let token = create_jwt(&user).ok()?;
            let claims = decode_jwt(&token).ok()?;

            Some((token, claims))
        }

        async fn refresh<'r>(req: &'r Request<'_>) -> Option<(String, Claims)> {
            for name in ["maremembermetoken", "remembermetoken"] {
                let Some(cookie) = req.cookies().get(name) else {
                    continue;
                };
                if let Some(res) = refresh_with_code(req, cookie.value()).await {
                    return Some(res);
                }
            }

            None
        }

        async fn get_token<'r>(req: &'r Request<'_>) -> Option<(String, bool)> {
            if let Some(token) = req.headers().get_one("authorization") {
                if validate_token(token).is_ok() {
                    return Some((token.to_string(), false));
                } else {
                    if let Some(jwt) = refresh_with_code(req, token).await {
                        return Some((jwt.0, false));
                    }
                }
            }

            for name in ["matoken", "token"] {
                if let Some(c) = req.cookies().get(name) {
                    return Some((c.value().to_string(), false));
                }
            }

            if let Some(token) = refresh(req).await {
                return Some((token.0, true));
            }

            None
        }

        let Some((token, readd_token)) = get_token(req).await else {
            return Outcome::Error((Status::Unauthorized, Status::Unauthorized));
        };

        match validate_token(&token) {
            Ok(claims) => Outcome::Success(JWT {
                claims,
                token: if readd_token { Some(token) } else { None },
            }),
            Err(_) => {
                if let Some((new_token, claims)) = refresh(req).await {
                    Outcome::Success(JWT {
                        claims,
                        token: Some(new_token),
                    })
                } else {
                    Outcome::Error((Status::Unauthorized, Status::Unauthorized))
                }
            }
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

    let expiration = SystemTime::now()
        .checked_add(Duration::from_secs(3600))
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let claims = Claims {
        sub: (*user.username).to_string(),
        email: user.email.clone(),
        perms: user.perms,
        exp: expiration as usize,
        iat: now as usize,
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

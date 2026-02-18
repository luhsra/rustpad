use std::{convert::Infallible, fmt, str::FromStr};

use axum::{
    RequestPartsExt,
    extract::{FromRequestParts, OptionalFromRequestParts},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::{TypedHeader, headers, typed_header::TypedHeaderRejectionReason};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as base64engine;
use rand::random;
use tracing::{error, warn};

use crate::auth::LOGGEDIN_EXPIRE_SEC;

// Use anyhow, define error and enable '?'
// For a simplified example of using anyhow in axum check /examples/anyhow-error-response
#[derive(Debug)]
pub struct AppError(pub anyhow::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!("Application error: {:#}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
// `Result<_, AppError>`. That way you don't need to do that manually.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

const SESSION_COOKIE: &str = "rustpad_session";

/// Unique identifier for a document or user.
#[repr(align(64))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier([u8; Self::MAX_LEN]);
impl Identifier {
    /// Maximum length of a document ID, in bytes.
    pub const MAX_LEN: usize = 64;
    fn valid_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ')
    }
}
impl FromStr for Identifier {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() > Self::MAX_LEN {
            anyhow::bail!("Document ID is too long");
        }
        if !s.chars().all(Self::valid_char) {
            anyhow::bail!("Document ID contains invalid characters");
        }
        let mut bytes = [0u8; Self::MAX_LEN];
        bytes[..s.len()].copy_from_slice(s.as_bytes());
        Ok(Self(bytes))
    }
}
impl AsRef<str> for Identifier {
    fn as_ref(&self) -> &str {
        let len = self.0.iter().position(|&b| b == 0).unwrap_or(Self::MAX_LEN);
        std::str::from_utf8(&self.0[..len]).expect("DocumentID contains invalid UTF-8")
    }
}
impl std::fmt::Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}
impl serde::Serialize for Identifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_ref())
    }
}
impl<'de> serde::Deserialize<'de> for Identifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// Session identifier.
#[repr(align(64))]
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct Session([u8; 64]);
impl Session {
    pub fn new() -> Self {
        Self(random())
    }
    fn from_cookie(cookie: &str) -> Option<Self> {
        let decoded = base64engine.decode(cookie).ok()?;
        let buf = decoded.try_into().ok()?;
        Some(Self(buf))
    }
    fn to_cookie(&self) -> String {
        format!(
            "{SESSION_COOKIE}={self}; Path=/; HttpOnly; Age={LOGGEDIN_EXPIRE_SEC}; SameSite=Lax"
        )
    }
    fn change_cookie(&self, cookie: HeaderValue, reply: impl IntoResponse) -> impl IntoResponse {
        let headers = HeaderMap::from_iter([(HeaderName::from_static("set-cookie"), cookie)]);
        (headers, reply)
    }
    pub fn set_cookie(&self, reply: impl IntoResponse) -> impl IntoResponse {
        self.change_cookie(self.to_cookie().parse().unwrap(), reply)
    }
    pub fn delete_cookie(&self, reply: impl IntoResponse) -> impl IntoResponse {
        let cookie = format!(
            "{SESSION_COOKIE}=deleted; Path=/; HttpOnly; Expires=Thu, 01 Jan 1970 00:00:00 GMT; SameSite=Lax"
        );
        self.change_cookie(cookie.parse().unwrap(), reply)
    }
}
impl fmt::Display for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&base64engine.encode(self.0))
    }
}

pub struct AuthRedirect;

impl IntoResponse for AuthRedirect {
    fn into_response(self) -> Response {
        Redirect::temporary("/auth/discord").into_response()
    }
}

impl<S> OptionalFromRequestParts<S> for Session
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        let cookies = match parts.extract::<TypedHeader<headers::Cookie>>().await {
            Ok(cookie) => cookie,
            Err(e) => {
                match *e.name() {
                    header::COOKIE => match e.reason() {
                        TypedHeaderRejectionReason::Missing => (),
                        _ => error!("unexpected error getting Cookie header(s): {e}"),
                    },
                    _ => error!("unexpected error getting cookies: {e}"),
                };
                return Ok(None);
            }
        };
        Ok(cookies.get(SESSION_COOKIE).and_then(Session::from_cookie))
    }
}

impl<S> FromRequestParts<S> for Session
where
    S: Send + Sync,
{
    type Rejection = AuthRedirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        warn!("extracting session from request...");
        <Self as OptionalFromRequestParts<S>>::from_request_parts(parts, state)
            .await
            .ok()
            .flatten()
            .ok_or(AuthRedirect)
    }
}

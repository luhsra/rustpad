use std::{fmt, str::FromStr};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as base64engine;
use log::{info, warn};
use rand::random;
use warp::{
    Filter,
    reject::Rejection,
    reply::{Reply, Response},
};

use crate::auth::LOGGEDIN_EXPIRE_SEC;

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
        info!("creating new session");
        Self(random())
    }
    fn from_cookie(cookie: &str) -> Option<Self> {
        info!("parsing session cookie: {}", cookie);
        let decoded = base64engine.decode(cookie).ok()?;
        let buf = decoded.try_into().ok()?;
        Some(Self(buf))
    }
    fn to_cookie(&self) -> String {
        format!(
            "{SESSION_COOKIE}={self}; Path=/; HttpOnly; Age={LOGGEDIN_EXPIRE_SEC}; SameSite=Lax"
        )
    }
}
impl fmt::Display for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&base64engine.encode(self.0))
    }
}

#[derive(Debug, Clone)]
pub struct SessionState {
    pub session: Session,
    pub new: bool,
}
impl SessionState {
    pub fn new(session_opt: Option<String>) -> Self {
        if let Some(cookie) = session_opt
            && let Some(session) = Session::from_cookie(&cookie)
        {
            Self {
                session,
                new: false,
            }
        } else {
            warn!("No valid session cookie found, creating new session");
            Self {
                session: Session::new(),
                new: true,
            }
        }
    }
    pub fn filter() -> impl Filter<Extract = (Self,), Error = Rejection> + Clone {
        warp::filters::cookie::optional(SESSION_COOKIE)
            .map(Self::new)
            .boxed()
    }
    pub fn attach_reply<R: Reply>(self, reply: R) -> Response {
        if self.new {
            let header = self.session.to_cookie();
            warp::reply::with_header(reply, "set-cookie", header).into_response()
        } else {
            reply.into_response()
        }
    }
}

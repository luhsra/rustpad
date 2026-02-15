//! Server backend for the Rustpad collaborative text editor.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use dashmap::DashMap;
use log::{error, info};
use rand::random_range;
use serde::Serialize;
use tokio::time::{self, Instant};
use warp::{Filter, Rejection, Reply, filters::BoxedFilter, ws::Ws};

use crate::{database::Database, rustpad::Rustpad};

pub mod database;
mod ot;
mod rustpad;

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

/// Server configuration, parsed from environment variables.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Number of days after which documents are garbage collected.
    pub expiry_days: u32,
    /// Database for document persistence.
    pub database: Database,
}
impl ServerConfig {
    /// Construct a new server configuration.
    pub async fn new(expiry_days: u32, storage: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            expiry_days,
            database: Database::new(storage).await?,
        })
    }
    /// Construct a new server configuration with a temporary database for testing.
    pub async fn temporary(expiry_days: u32) -> anyhow::Result<Self> {
        Ok(Self {
            expiry_days,
            database: Database::temporary().await?,
        })
    }
    /// Load server configuration from environment variables.
    pub async fn from_env() -> anyhow::Result<Self> {
        let expiry_days = std::env::var("EXPIRY_DAYS")
            .unwrap_or_else(|_| String::from("1"))
            .parse()
            .context("Unable to parse EXPIRY_DAYS")?;
        let storage = std::env::var("STORAGE")
            .unwrap_or_else(|_| String::from("storage"))
            .into();
        Self::new(expiry_days, storage).await
    }
}

/// An entry stored in the global server map.
///
/// Each entry corresponds to a single document. This is garbage collected by a
/// background task after one day of inactivity, to avoid server memory usage
/// growing without bound.
struct Document {
    last_accessed: Instant,
    rustpad: Arc<Rustpad>,
}

impl Document {
    fn new(rustpad: Arc<Rustpad>) -> Self {
        Self {
            last_accessed: Instant::now(),
            rustpad,
        }
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        self.rustpad.kill();
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct CustomReject(anyhow::Error);

impl warp::reject::Reject for CustomReject {}

/// The shared state of the server, accessible from within request handlers.
#[derive(Clone)]
struct ServerState {
    /// Concurrent map storing in-memory documents.
    documents: Arc<DashMap<Identifier, Document>>,
    /// Connection to the database pool, if persistence is enabled.
    database: Database,
}

/// Statistics about the server, returned from an API endpoint.
#[derive(Serialize)]
struct Stats {
    /// System time when the server started, in seconds since Unix epoch.
    start_time: u64,
    /// Number of documents currently tracked by the server.
    num_documents: usize,
    /// Number of documents persisted in the database.
    database_size: usize,
}

/// A combined filter handling all server routes.
pub fn server(config: ServerConfig) -> BoxedFilter<(impl Reply,)> {
    warp::path("api")
        .and(backend(config.database, config.expiry_days))
        .or(frontend())
        .boxed()
}

/// Construct routes for static files from React.
fn frontend() -> BoxedFilter<(impl Reply,)> {
    warp::fs::dir("dist").boxed()
}

/// Construct backend routes, including WebSocket handlers.
fn backend(database: Database, expiry_days: u32) -> BoxedFilter<(impl Reply,)> {
    let state = ServerState {
        documents: Default::default(),
        database,
    };
    tokio::spawn(cleaner(state.clone(), expiry_days));

    let state_filter = warp::any().map(move || state.clone());

    let socket = warp::path!("socket" / Identifier)
        .and(warp::ws())
        .and(state_filter.clone())
        .and_then(socket_handler);

    let text = warp::path!("text" / Identifier)
        .and(state_filter.clone())
        .and_then(text_handler);

    let start_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("SystemTime returned before UNIX_EPOCH")
        .as_secs();
    let stats = warp::path!("stats")
        .and(warp::any().map(move || start_time))
        .and(state_filter)
        .and_then(stats_handler);

    socket.or(text).or(stats).boxed()
}

/// Handler for the `/api/socket/{id}` endpoint.
async fn socket_handler(
    id: Identifier,
    ws: Ws,
    state: ServerState,
) -> Result<impl Reply, Rejection> {
    use dashmap::mapref::entry::Entry;

    info!("socket connection for id = {}", id);

    let mut entry = match state.documents.entry(id.clone()) {
        Entry::Occupied(e) => e.into_ref(),
        Entry::Vacant(e) => {
            let rustpad = if let Ok(document) = state.database.load_document(&id).await {
                Arc::new(Rustpad::load(document).await)
            } else {
                Arc::new(Rustpad::default())
            };
            tokio::spawn(persister(id, Arc::clone(&rustpad), state.database.clone()));
            e.insert(Document::new(rustpad))
        }
    };

    let value = entry.value_mut();
    value.last_accessed = Instant::now();
    let rustpad = Arc::clone(&value.rustpad);
    Ok(ws.on_upgrade(|socket| async move { rustpad.on_connection(socket).await }))
}

/// Handler for the `/api/text/{id}` endpoint.
async fn text_handler(id: Identifier, state: ServerState) -> Result<impl Reply, Rejection> {
    Ok(match state.documents.get(&id) {
        Some(value) => value.rustpad.text().await,
        None => state
            .database
            .load_document(&id)
            .await
            .map(|document| document.text)
            .unwrap_or_default(),
    })
}

/// Handler for the `/api/stats` endpoint.
async fn stats_handler(start_time: u64, state: ServerState) -> Result<impl Reply, Rejection> {
    let num_documents = state.documents.len();
    let database_size = match state.database.document_count().await {
        Ok(size) => size,
        Err(e) => return Err(warp::reject::custom(CustomReject(e))),
    };
    Ok(warp::reply::json(&Stats {
        start_time,
        num_documents,
        database_size,
    }))
}

const HOUR: Duration = Duration::from_secs(3600);

/// Reclaims memory for documents.
async fn cleaner(state: ServerState, expiry_days: u32) {
    loop {
        time::sleep(HOUR).await;
        let mut keys = Vec::new();
        for entry in &*state.documents {
            if entry.last_accessed.elapsed() > HOUR * 24 * expiry_days {
                keys.push(entry.key().clone());
            }
        }
        info!("cleaner removing keys: {:?}", keys);
        for key in keys {
            state.documents.remove(&key);
        }
    }
}

const PERSIST_INTERVAL: Duration = Duration::from_secs(3);
const PERSIST_INTERVAL_JITTER: Duration = Duration::from_secs(1);

/// Persists changed documents after a fixed time interval.
async fn persister(id: Identifier, rustpad: Arc<Rustpad>, db: Database) {
    let mut last_revision = 0;
    while !rustpad.killed() {
        let interval = PERSIST_INTERVAL + random_range(Duration::ZERO..=PERSIST_INTERVAL_JITTER);
        time::sleep(interval).await;
        let revision = rustpad.revision().await;
        if revision > last_revision {
            info!("persisting revision {} for id = {}", revision, id);
            if let Err(e) = db.store_document(&id, &rustpad.snapshot().await).await {
                error!("when persisting document {}: {}", id, e);
            } else {
                last_revision = revision;
            }
        }
    }
}

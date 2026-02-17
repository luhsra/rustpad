//! Server backend for the Rustpad collaborative text editor.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use dashmap::{DashMap, Entry};
use log::{error, info};
use rand::random_range;
use serde::Serialize;
use tokio::sync::Notify;
use tokio::time::{self, Instant};
use warp::reply::Response;
use warp::{Filter, Rejection, Reply, filters::BoxedFilter, ws::Ws};

mod auth;
pub mod database;
use database::Database;
mod ot;
mod rustpad;
use rustpad::Rustpad;
mod util;
use util::Identifier;

use crate::rustpad::UserInfo;
use crate::util::{Session, SessionState};

/// Server configuration, parsed from environment variables.
#[derive(Debug)]
pub struct ServerConfig {
    /// Number of days after which documents are garbage collected.
    pub expiry_days: u32,
    /// Database for document persistence.
    pub database: Database,
    /// OpenID Connect configuration, if authentication is enabled.
    pub openid: Option<auth::OpenIdState>,
}
impl ServerConfig {
    /// Construct a new server configuration.
    pub async fn new(
        expiry_days: u32,
        storage: PathBuf,
        openid: Option<auth::OpenIdConfig>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            expiry_days,
            database: Database::new(storage).await?,
            openid: if let Some(config) = openid {
                Some(auth::OpenIdState::new(config).await?)
            } else {
                None
            },
        })
    }
    /// Construct a new server configuration with a temporary database for testing.
    pub async fn temporary(expiry_days: u32) -> anyhow::Result<Self> {
        Ok(Self {
            expiry_days,
            database: Database::temporary().await?,
            openid: None,
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
        let openid = if let Ok(config) = std::env::var("OPENID_CONFIG") {
            Some(
                serde_json::from_str(&tokio::fs::read_to_string(&config).await?)
                    .context("Unable to parse OPENID_CONFIG")?,
            )
        } else {
            error!("OPENID_CONFIG not set, authentication will be disabled");
            None
        };
        Self::new(expiry_days, storage, openid).await
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
struct ServerState {
    /// Concurrent map storing in-memory documents.
    documents: DashMap<Identifier, Document>,
    /// Connection to the database pool, if persistence is enabled.
    database: Database,
    /// User sessions for authentication, if enabled.
    users: auth::UserSessions,
    /// Used to notify the persister task to continue persisting documents.
    notify_persister: Notify,
}
impl Drop for ServerState {
    fn drop(&mut self) {
        info!("shutting down, saving documents...");
        for entry in &self.documents {
            let (id, value) = entry.pair();
            if let Some(snapshot) = value.rustpad.dirty_snapshot_blocking() {
                info!("persisting document {id}");
                if let Err(e) = self.database.store_document_blocking(id, &snapshot) {
                    error!("Error persisting document {id}: {e:?}");
                }
            }
        }
    }
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
    /// User name
    user: Option<String>,
    /// Whether the user is an admin
    admin: bool,
}

/// A combined filter handling all server routes.
pub fn server(config: ServerConfig) -> BoxedFilter<(impl Reply,)> {
    warp::path("api")
        .and(backend(config))
        .or(frontend())
        .boxed()
}

/// Construct routes for static files from React.
fn frontend() -> impl Filter<Extract = (impl Reply,), Error = Rejection> + Clone {
    warp::fs::dir("dist").map(Reply::into_response).boxed()
}

/// Construct backend routes, including WebSocket handlers.
fn backend(
    config: ServerConfig,
) -> impl Filter<Extract = (impl Reply,), Error = Rejection> + Clone {
    let ServerConfig {
        expiry_days,
        database,
        openid,
    } = config;
    let state = Arc::new(ServerState {
        documents: Default::default(),
        database,
        users: auth::UserSessions::new(openid),
        notify_persister: Notify::new(),
    });

    tokio::spawn(persister(state.clone()));
    let state_filter = warp::any().map(move || state.clone());

    let socket = SessionState::filter()
        .and(warp::path!("socket" / Identifier))
        .and(warp::ws())
        .and(state_filter.clone())
        .and_then(
            async |ss: SessionState, id, ws, s| -> Result<_, Rejection> {
                Ok((ss.clone(), socket_handler(id, ss.session, ws, s).await?))
            },
        );

    let text = SessionState::filter()
        .and(warp::path!("text" / Identifier))
        .and(state_filter.clone())
        .and_then(async |ss: SessionState, id, s| -> Result<_, Rejection> {
            Ok((ss.clone(), text_handler(id, ss.session, s).await?))
        });

    let start_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("SystemTime returned before UNIX_EPOCH")
        .as_secs();
    let stats = SessionState::filter()
        .and(warp::path!("stats"))
        .and(warp::any().map(move || start_time))
        .and(state_filter.clone())
        .and_then(async |ss: SessionState, t, s| -> Result<_, Rejection> {
            Ok((ss.clone(), stats_handler(ss.session, t, s).await?))
        });

    let login = SessionState::filter()
        .and(warp::path("login"))
        .and(state_filter.clone())
        .and(warp::query::<auth::LoginQuery>())
        .and_then(async |ss: SessionState, s, q| -> Result<_, Rejection> {
            Ok((ss.clone(), auth::login(s, ss.session, q).await?))
        });

    let authorized = SessionState::filter()
        .and(warp::path("authorized"))
        .and(warp::get())
        .and(state_filter.clone())
        .and(warp::query::<auth::AuthorizedQuery>())
        .and_then(async |ss: SessionState, s, q| -> Result<_, Rejection> {
            Ok((ss.clone(), auth::authorized(s, ss.session, q).await?))
        });

    let logout = SessionState::filter()
        .and(warp::path("logout"))
        .and(warp::get())
        .and(state_filter.clone())
        .and_then(async |ss: SessionState, s| -> Result<_, Rejection> {
            Ok((ss.clone(), auth::logout(s, ss.session).await?))
        });

    socket
        .or(text)
        .unify()
        .or(stats)
        .unify()
        .or(login)
        .unify()
        .or(authorized)
        .unify()
        .or(logout)
        .unify()
        .untuple_one()
        .map(SessionState::attach_reply)
}

/// Handler for the `/api/socket/{id}` endpoint.
async fn socket_handler(
    id: Identifier,
    session: Session,
    ws: Ws,
    state: Arc<ServerState>,
) -> Result<Response, Rejection> {
    use dashmap::mapref::entry::Entry;

    let user = state.users.get_user(&session).await;
    let is_admin = user.as_ref().map(|u| u.admin).unwrap_or(false);

    info!("socket connection for id = {id}");

    let mut entry = match state.documents.entry(id.clone()) {
        Entry::Occupied(e) => {
            let document = e.into_ref();
            if document.rustpad.is_limited().await && !is_admin {
                info!("denying access to closed document {id}");
                return Ok(warp::http::StatusCode::FORBIDDEN.into_response());
            }
            document
        }
        Entry::Vacant(e) => {
            let rustpad = if let Ok(document) = state.database.load_document(&id).await {
                if document.meta.limited && !is_admin {
                    info!("denying access to closed document {id}");
                    return Ok(warp::http::StatusCode::FORBIDDEN.into_response());
                }

                Arc::new(Rustpad::load(document).await)
            } else {
                Arc::new(Rustpad::default())
            };
            let inserted = e.insert(Document::new(rustpad));
            // Wakeup if the persister is sleeping
            state.notify_persister.notify_waiters();
            inserted
        }
    };

    let value = entry.value_mut();
    let rustpad = Arc::clone(&value.rustpad);
    value.last_accessed = Instant::now();
    let upgrade = ws.on_upgrade(async move |socket| {
        rustpad
            .on_connection(socket, user.map(UserInfo::from))
            .await
    });
    Ok(upgrade.into_response())
}

/// Handler for the `/api/text/{id}` endpoint.
async fn text_handler(
    id: Identifier,
    session: Session,
    state: Arc<ServerState>,
) -> Result<Response, Rejection> {
    let document = match state.documents.get(&id) {
        Some(value) => Some(value.rustpad.snapshot().await),
        None => state.database.load_document(&id).await.ok(),
    };
    if let Some(document) = document {
        info!(
            "text request for id = {id}, limited = {}",
            document.meta.limited
        );
        if document.meta.limited {
            if let Some(user) = state.users.get_user(&session).await
                && user.admin
            {
                info!("access {} -> {id}", user.name);
            } else {
                return Ok(warp::http::StatusCode::FORBIDDEN.into_response());
            }
        }
        return Ok(warp::reply::with_header(
            document.text,
            "Language",
            document.meta.language.clone(),
        )
        .into_response());
    }
    Ok(warp::reply().into_response())
}

/// Handler for the `/api/stats` endpoint.
async fn stats_handler(
    session: Session,
    start_time: u64,
    state: Arc<ServerState>,
) -> Result<Response, Rejection> {
    let num_documents = state.documents.len();
    let database_size = match state.database.document_count().await {
        Ok(size) => size,
        Err(e) => return Err(warp::reject::custom(CustomReject(e))),
    };
    let user = state.users.get_user(&session).await;
    Ok(warp::reply::json(&Stats {
        start_time,
        num_documents,
        database_size,
        user: user.as_ref().map(|u| u.name.clone()),
        admin: user.as_ref().map(|u| u.admin).unwrap_or(false),
    })
    .into_response())
}

const PERSIST_INTERVAL: Duration = Duration::from_secs(10);
const PERSIST_INTERVAL_JITTER: Duration = Duration::from_secs(6);

/// Persists changed documents after a fixed time interval.
async fn persister(state: Arc<ServerState>) {
    loop {
        info!("checking for documents to persist...");

        let mut to_persist = Vec::new();
        for entry in &state.documents {
            let (id, value) = entry.pair();
            if let Some(snapshot) = value.rustpad.dirty_snapshot().await {
                to_persist.push((id.clone(), snapshot));
            }
        }

        let mut jitter =
            Duration::from_millis(random_range(0..PERSIST_INTERVAL_JITTER.as_millis() as u64));
        if to_persist.is_empty() {
            // Wait a bit longer if there are no documents to persist
            jitter += PERSIST_INTERVAL;
        }

        // Persist documents outside of the loop to avoid holding locks while doing I/O
        for (id, snapshot) in to_persist {
            info!("persisting document {id}");
            if let Err(e) = state.database.store_document(&id, &snapshot).await {
                error!("Error persisting document {id}: {e:?}");
            } else {
                // Remove idle documents from memory
                if let Entry::Occupied(e) = state.documents.entry(id.clone()) {
                    if e.get().rustpad.kill_if_idle().await {
                        info!("removing document {id} from memory");
                        e.remove();
                    }
                }
            }
        }

        while state.documents.is_empty() {
            // If there are no documents, sleep until the next one is created to avoid unnecessary wakeups
            state.notify_persister.notified().await;
        }

        time::sleep(PERSIST_INTERVAL + jitter).await;
    }
}

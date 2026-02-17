//! Server backend for the Rustpad collaborative text editor.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use dashmap::DashMap;
use log::{error, info};
use rand::random_range;
use serde::Serialize;
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
use crate::util::SessionState;

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
}
impl Drop for ServerState {
    fn drop(&mut self) {
        info!("shutting down, saving documents...");
        futures::executor::block_on(async {
            for entry in &self.documents {
                let (id, value) = entry.pair();
                info!(
                    "persisting document {id} with revision {}",
                    value.rustpad.revision().await
                );
                if let Err(e) = self
                    .database
                    .store_document_blocking(id, &value.rustpad.snapshot().await)
                {
                    error!("Error persisting document {id}: {e:?}");
                }
            }
        });
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
pub fn server(config: ServerConfig) -> BoxedFilter<(Response,)> {
    SessionState::filter()
        .and(
            warp::path("api")
                .and(backend(config))
                .or(frontend())
                .unify()
                .boxed(),
        )
        .map(|session: SessionState, reply: Response| session.attach_reply(reply))
        .boxed()
}

/// Construct routes for static files from React.
fn frontend() -> BoxedFilter<(Response,)> {
    warp::fs::dir("dist").map(Reply::into_response).boxed()
}

/// Construct backend routes, including WebSocket handlers.
fn backend(config: ServerConfig) -> BoxedFilter<(Response,)> {
    let ServerConfig {
        expiry_days,
        database,
        openid,
    } = config;
    let state = Arc::new(ServerState {
        documents: Default::default(),
        database,
        users: auth::UserSessions::new(openid),
    });

    let state_filter = warp::any().map(move || state.clone());

    let socket = warp::path!("socket" / Identifier)
        .and(SessionState::filter())
        .and(warp::ws())
        .and(state_filter.clone())
        .and_then(socket_handler);

    let text = warp::path!("text" / Identifier)
        .and(SessionState::filter())
        .and(state_filter.clone())
        .and_then(text_handler);

    let start_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("SystemTime returned before UNIX_EPOCH")
        .as_secs();
    let stats = warp::path!("stats")
        .and(SessionState::filter())
        .and(warp::any().map(move || start_time))
        .and(state_filter.clone())
        .and_then(stats_handler);

    let login = warp::path("login")
        .and(SessionState::filter())
        .and(warp::get())
        .and(state_filter.clone())
        .and(warp::query::<auth::LoginQuery>())
        .and_then(
            async move |session: SessionState, state, query| -> Result<_, Rejection> {
                auth::login(state, session.session.clone(), query)
                    .await
                    .map(|reply| reply.into_response())
            },
        );

    let authorized = warp::path("authorized")
        .and(warp::get())
        .and(SessionState::filter())
        .and(warp::query::<auth::AuthorizedQuery>())
        .and(state_filter.clone())
        .and_then(
            async move |session: SessionState, query, state| -> Result<_, Rejection> {
                auth::authorized(state, session.session.clone(), query)
                    .await
                    .map(|reply| reply.into_response())
            },
        );

    let logout = warp::path("logout")
        .and(warp::get())
        .and(SessionState::filter())
        .and(state_filter.clone())
        .and_then(
            async move |session: SessionState, state| -> Result<_, Rejection> {
                auth::logout(state, session.session)
                    .await
                    .map(|reply| reply.into_response())
            },
        );

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
        .boxed()
}

/// Handler for the `/api/socket/{id}` endpoint.
async fn socket_handler(
    id: Identifier,
    session: SessionState,
    ws: Ws,
    state: Arc<ServerState>,
) -> Result<Response, Rejection> {
    use dashmap::mapref::entry::Entry;

    let user = state.users.get_user(&session.session).await;
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
            tokio::spawn(persister(id.clone(), Arc::clone(&rustpad), state.clone()));
            e.insert(Document::new(rustpad))
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
    session: SessionState,
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
            if let Some(user) = state.users.get_user(&session.session).await
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
    session: SessionState,
    start_time: u64,
    state: Arc<ServerState>,
) -> Result<Response, Rejection> {
    let num_documents = state.documents.len();
    let database_size = match state.database.document_count().await {
        Ok(size) => size,
        Err(e) => return Err(warp::reject::custom(CustomReject(e))),
    };
    let user = state.users.get_user(&session.session).await;
    Ok(warp::reply::json(&Stats {
        start_time,
        num_documents,
        database_size,
        user: user.as_ref().map(|u| u.name.clone()),
        admin: user.as_ref().map(|u| u.admin).unwrap_or(false),
    })
    .into_response())
}

const PERSIST_INTERVAL: Duration = Duration::from_secs(3);
const PERSIST_INTERVAL_JITTER: Duration = Duration::from_secs(1);

/// Persists changed documents after a fixed time interval.
async fn persister(id: Identifier, rustpad: Arc<Rustpad>, state: Arc<ServerState>) {
    let mut last_revision = 0;
    while !rustpad.killed() {
        let interval = PERSIST_INTERVAL + random_range(Duration::ZERO..=PERSIST_INTERVAL_JITTER);
        time::sleep(interval).await;
        let revision = rustpad.revision().await;

        // TODO: only persist if there is any content!
        // TODO: version history
        // TODO: remove from memory after persisting, if no one edits, and reload on demand

        if revision > last_revision {
            info!("persisting revision {} for id = {}", revision, id);
            if let Err(e) = state
                .database
                .store_document(&id, &rustpad.snapshot().await)
                .await
            {
                error!("when persisting document {}: {}", id, e);
            } else {
                last_revision = revision;
            }
        }
    }
}

//! Server backend for the Rustpad collaborative text editor.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use axum::extract::ws::Message;
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use dashmap::{DashMap, Entry};
use futures::SinkExt;
use rand::random_range;
use serde::Serialize;
use tokio::sync::{Notify, broadcast};
use tokio::time::{self, Instant};
use tracing::{debug, error, info};

mod auth;
pub mod database;
use database::Database;
mod ot;
pub mod rustpad;
use rustpad::Rustpad;
mod util;
use tower_http::services::{ServeDir, ServeFile};
use util::Identifier;
mod collab;

use crate::auth::User;
use crate::rustpad::{ClientMsg, Role, Visibility};
use crate::util::{AppError, Session};

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

#[derive(Debug, Clone)]
enum GlobalMsg {
    UserUpdate(User),
}

/// The shared state of the server, accessible from within request handlers.
pub struct ServerState {
    /// Concurrent map storing in-memory documents.
    documents: DashMap<Identifier, Document>,

    new_documents: DashMap<Identifier, Arc<collab::Document>>,
    /// Connection to the database pool, if persistence is enabled.
    database: Database,
    /// User sessions for authentication, if enabled.
    users: Option<Arc<auth::UserSessions>>,
    /// Used to notify the persister task to continue persisting documents.
    notify_persister: Notify,
    /// System time when the server started, in seconds since Unix epoch.
    start_time: SystemTime,
    /// Broadcast channel for global messages like user updates
    update: broadcast::Sender<GlobalMsg>,
}
impl ServerState {
    /// Construct a new server configuration.
    pub async fn new(storage: PathBuf, openid: Option<auth::OpenIdConfig>) -> anyhow::Result<Self> {
        Ok(Self {
            database: Database::new(storage).await?,
            users: if let Some(config) = openid {
                Some(Arc::new(auth::UserSessions::new(config).await?))
            } else {
                None
            },
            new_documents: DashMap::new(),
            documents: DashMap::new(),
            notify_persister: Notify::new(),
            start_time: SystemTime::now(),
            update: broadcast::channel(16).0,
        })
    }
    /// Construct a new server configuration with a temporary database for testing.
    pub async fn temporary() -> anyhow::Result<Self> {
        Ok(Self {
            new_documents: DashMap::new(),
            database: Database::temporary().await?,
            users: None,
            documents: DashMap::new(),
            notify_persister: Notify::new(),
            start_time: SystemTime::now(),
            update: broadcast::channel(16).0,
        })
    }
    /// Load server configuration from environment variables.
    pub async fn from_env() -> anyhow::Result<Self> {
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
        Self::new(storage, openid).await
    }

    /// Get the user info for the given session, if authentication is enabled.
    async fn get_user(&self, session: &Session) -> Option<User> {
        self.users.as_ref()?.get_user(session).await
    }

    pub async fn persist(&self) {
        info!("persisting documents...");
        for entry in &self.documents {
            let (id, value) = entry.pair();
            if let Some(snapshot) = value.rustpad.dirty_snapshot().await {
                info!("persisting document {id}");
                if let Err(e) = self.database.store_document(id, &snapshot).await {
                    error!("Error persisting document {id}: {e:?}");
                }
            }
        }
    }
}

/// A combined filter handling all server routes.
pub fn server(state: Arc<ServerState>) -> Router {
    tokio::spawn(persister(state.clone()));

    Router::new()
        .nest(
            "/api",
            Router::new()
                .route("/socket/{id}", any(socket_handler))
                .route("/collab/{id}", get(peer_handler))
                .route("/text/{id}", get(text_handler))
                .route("/stats", get(stats_handler))
                .with_state(state.clone()),
        )
        .nest("/auth", auth::routes(state.users.clone()))
        .route_service("/new", ServeFile::new("dist/new.html"))
        .route_service("/", ServeFile::new("dist/index.html"))
        .fallback_service(ServeDir::new("dist"))
        .layer(tower_http::trace::TraceLayer::new_for_http())
}

async fn peer_handler(
    Path(id): Path<Identifier>,
    session: Option<Session>,
    State(state): State<Arc<ServerState>>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    info!("collab connection for id = {id}");

    let document = match state.new_documents.entry(id.clone()) {
        Entry::Occupied(e) => e.into_ref(),
        Entry::Vacant(e) => {
            let persisted = state.database.load_document(&id).await.unwrap_or_default();
            e.insert(Arc::new(collab::Document::new(persisted.text).await))
        }
    }
    .clone();

    let upgrade = ws.on_upgrade(move |socket| collab::peer(socket, document));
    Ok(upgrade.into_response())
}

/// Handler for the `/api/socket/{id}` endpoint.
async fn socket_handler(
    Path(id): Path<Identifier>,
    session: Option<Session>,
    State(state): State<Arc<ServerState>>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    use dashmap::mapref::entry::Entry;

    let user = if let Some(session) = &session {
        state.get_user(session).await
    } else {
        None
    };

    let role = user
        .as_ref()
        .map(|u| if u.admin { Role::Admin } else { Role::User })
        .unwrap_or(Role::Anon);

    info!("socket connection for id = {id}");

    let mut entry = match state.documents.entry(id.clone()) {
        Entry::Occupied(e) => {
            let document = e.into_ref();
            if !role.can_access(document.rustpad.visibility().await) {
                info!("denying access to limited document {id}");
                return Ok(StatusCode::FORBIDDEN.into_response());
            }
            document
        }
        Entry::Vacant(e) => {
            let rustpad = if let Ok(document) = state.database.load_document(&id).await {
                if !role.can_access(document.meta.visibility) {
                    info!("denying access to limited document {id}");
                    return Ok(StatusCode::FORBIDDEN.into_response());
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

    let rustpad = {
        let value = entry.value_mut();
        value.last_accessed = Instant::now();
        value.rustpad.clone()
    };
    let state = state.clone();
    let id = id.clone();
    let upgrade =
        ws.on_upgrade(move |socket| websocket_connection(id, rustpad, socket, state, session));
    Ok(upgrade.into_response())
}

async fn websocket_connection(
    doc_id: Identifier,
    rustpad: Arc<Rustpad>,
    mut socket: axum::extract::ws::WebSocket,
    state: Arc<ServerState>,
    session: Option<Session>,
) {
    let mut user = if let Some(session) = &session {
        state.get_user(session).await
    } else {
        None
    };
    let role = user
        .as_ref()
        .map(|u| if u.admin { Role::Admin } else { Role::User })
        .unwrap_or(Role::Anon);

    let (user_id, mut revision, messages) = rustpad.init_connection(user.clone()).await;
    // TODO: use try block if stable
    let result = async |
        doc_id,
        rustpad: Arc<Rustpad>,
        socket: &mut axum::extract::ws::WebSocket,
        state: Arc<ServerState>,
        session
    | -> anyhow::Result<()> {
        for message in messages {
            debug!("socket {doc_id} - {user_id} -> {message:?}");
            socket.send(message.into()).await?;
        }

        let mut global_update_rx = state.update.subscribe();
        let mut doc_update_rx = rustpad.subscribe();

        loop {
            // In order to avoid the "lost wakeup" problem, we first request a
            // notification, **then** check the current state for new revisions.
            // This is the same approach that `tokio::sync::watch` takes.
            let notified = rustpad.notified();

            if rustpad.killed() {
                break;
            }
            if !role.can_access(rustpad.visibility().await) {
                info!("{doc_id} disconnecting users without permission");
                break;
            }
            if rustpad.revision().await > revision {
                let (new_revision, message) = rustpad.send_history(revision).await?;
                revision = new_revision;
                debug!("socket {doc_id} - {user_id} -> {message:?}");
                socket.send(message.into()).await?;
            }

            tokio::select! {
                _ = notified => {}
                update = global_update_rx.recv() => {
                    match update? {
                        GlobalMsg::UserUpdate(updated_user) => {
                            if let Some(user) = &mut user && user.name == updated_user.name {
                                info!("updating user {} info for document {doc_id}", user.name);
                                *user = updated_user;
                                rustpad.update_user(user.clone().into()).await;
                            }
                        }
                    }
                }
                update = doc_update_rx.recv() => {
                    let message = update?;
                    debug!("socket {doc_id} - {user_id} -> {message:?}");
                    socket.send(message.into()).await?;
                }
                result = socket.recv() => match result {
                    None => break,
                    Some(Ok(Message::Text(message))) => {
                        let message = serde_json::from_str(&message).context("Failed to parse JSON message")?;
                        debug!("socket {doc_id} - {user_id} <- {message:?}");
                        if let Some(user) = &mut user && let ClientMsg::ClientInfo { hue, .. } = &message {
                            user.hue = *hue;
                            if let Some(session) = &session && let Some(users) = &state.users {
                                // Update user info in session store as well
                                users.update_user(session, user.clone()).await;
                                state.update.send(GlobalMsg::UserUpdate(user.clone())).ok();
                            }
                        }
                        rustpad.handle_message(user_id, message, &user).await?;
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(m)) => {
                        debug!("socket {doc_id} - {user_id} received unsupported message: {m:?}");
                    }
                    Some(Err(e)) => {
                        error!("Error receiving websocket message for document {doc_id}: {e:?}");
                        break;
                    }
                }
            }
        }
        Ok(())
    }(doc_id.clone(), rustpad.clone(), &mut socket, state, session).await;

    rustpad.close_connection(user_id).await;
    socket.close().await.ok();

    if let Err(e) = result {
        error!("Error in websocket connection for document {doc_id}: {e:?}");
    }
}

/// Handler for the `/api/text/{id}` endpoint.
async fn text_handler(
    Path(id): Path<Identifier>,
    session: Option<Session>,
    State(state): State<Arc<ServerState>>,
) -> Result<impl IntoResponse, AppError> {
    let document = match state.documents.get(&id) {
        Some(value) => Some(value.rustpad.snapshot().await),
        None => state.database.load_document(&id).await.ok(),
    };
    if let Some(document) = document {
        info!(
            "text request for id = {id}, visibility = {:?}",
            document.meta.visibility
        );
        if document.meta.visibility != Visibility::Public {
            if let Some(session) = &session
                && let Some(user) = state.get_user(session).await
                && (document.meta.visibility == Visibility::Internal || user.admin)
            {
                info!("access {} -> {id}", user.name);
            } else {
                return Ok(StatusCode::FORBIDDEN.into_response());
            }
        }
        return Ok(Response::builder()
            .header("Language", document.meta.language.clone())
            .body(document.text)?
            .into_response());
    }
    Ok(().into_response())
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

/// Handler for the `/api/stats` endpoint.
async fn stats_handler(
    session: Option<Session>,
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Stats>, AppError> {
    let num_documents = state.documents.len();
    let database_size = state.database.document_count().await?;
    let user = if let Some(session) = &session {
        state.get_user(session).await
    } else {
        None
    };
    Ok(Json(Stats {
        start_time: state
            .start_time
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs(),
        num_documents,
        database_size,
        user: user.as_ref().map(|u| u.name.clone()),
        admin: user.is_some_and(|u| u.admin),
    }))
}

const PERSIST_INTERVAL: Duration = Duration::from_secs(10);
const PERSIST_INTERVAL_JITTER: Duration = Duration::from_secs(6);

/// Persists changed documents after a fixed time interval.
async fn persister(state: Arc<ServerState>) {
    loop {
        let mut to_persist = Vec::new();
        for entry in &state.documents {
            let (id, value) = entry.pair();
            to_persist.push((id.clone(), value.rustpad.dirty_snapshot().await));
        }

        let mut jitter =
            Duration::from_millis(random_range(0..PERSIST_INTERVAL_JITTER.as_millis() as u64));
        if to_persist.is_empty() {
            // Wait a bit longer if there are no documents to persist
            jitter += PERSIST_INTERVAL;
        }

        // Persist documents outside of the loop to avoid holding locks while doing I/O
        for (id, snapshot) in to_persist {
            if snapshot.is_some() {
                info!("persisting document {id}");
            }
            if let Some(snapshot) = snapshot
                && let Err(e) = state.database.store_document(&id, &snapshot).await
            {
                error!("Error persisting document {id}: {e:?}");
            } else {
                // Remove idle documents from memory
                if let Entry::Occupied(e) = state.documents.entry(id.clone())
                    && e.get().rustpad.kill_if_idle().await
                {
                    info!("removing document {id} from memory");
                    e.remove();
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

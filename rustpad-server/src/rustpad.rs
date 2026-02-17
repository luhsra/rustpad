//! Eventually consistent server-side logic for Rustpad.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use axum::extract::ws::{Message, WebSocket};
use futures::prelude::*;
use operational_transform::OperationSeq;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, RwLock, broadcast};
use tracing::{info, warn};

use crate::{database::PersistedDocument, ot::transform_index};

/// The main object representing a collaborative session.
pub struct Rustpad {
    /// State modified by critical sections of the code.
    state: RwLock<State>,
    /// Incremented to obtain unique user IDs.
    count: AtomicU64,
    /// Used to notify clients of new text operations.
    notify: Notify,
    /// Used to inform all clients of metadata updates.
    update: broadcast::Sender<ServerMsg>,
    /// Set to true when the document is destroyed.
    killed: AtomicBool,
}

/// Shared state involving multiple users, protected by a lock.
struct State {
    operations: Vec<UserOperation>,
    text: String,
    meta: DocumentMeta,
    users: HashMap<u64, UserInfo>,
    cursors: HashMap<u64, CursorData>,
    dirty: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            text: String::new(),
            meta: DocumentMeta {
                language: "markdown".to_string(),
                limited: false,
            },
            users: HashMap::new(),
            cursors: HashMap::new(),
            dirty: false,
        }
    }
}

/// Metadata for a persisted document.
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct DocumentMeta {
    /// Language of the document for editor syntax highlighting.
    pub language: String,
    /// If accessible by external users.
    pub limited: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct UserOperation {
    id: u64,
    operation: OperationSeq,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub name: String,
    pub hue: u16,
    #[serde(default)]
    pub admin: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CursorData {
    cursors: Vec<u32>,
    selections: Vec<(u32, u32)>,
}

/// A message received from the client over WebSocket.
#[derive(Clone, Debug, Serialize, Deserialize)]
enum ClientMsg {
    /// Represents a sequence of local edits from the user.
    Edit {
        revision: usize,
        operation: OperationSeq,
    },
    /// Sets the metadata of the editor.
    SetMeta {
        language: Option<String>,
        limited: Option<bool>,
    },
    /// Sets the user's current information.
    ClientInfo(UserInfo),
    /// Sets the user's cursor and selection positions.
    CursorData(CursorData),
}

/// A message sent to the client over WebSocket.
#[derive(Clone, Debug, Serialize, Deserialize)]
enum ServerMsg {
    /// Informs the client of their unique socket ID and admin status.
    Identity { id: u64, info: Option<UserInfo> },
    /// Broadcasts text operations to all clients.
    History {
        start: usize,
        operations: Vec<UserOperation>,
    },
    /// Broadcasts the current metadata, last writer wins.
    Meta { language: String, limited: bool },
    /// Broadcasts a user's information, or `None` on disconnect.
    UserInfo { id: u64, info: Option<UserInfo> },
    /// Broadcasts a user's cursor position.
    UserCursor { id: u64, data: CursorData },
}

impl From<ServerMsg> for Message {
    fn from(msg: ServerMsg) -> Self {
        let serialized = serde_json::to_string(&msg).expect("failed serialize");
        Message::text(serialized)
    }
}

impl Default for Rustpad {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            state: Default::default(),
            count: Default::default(),
            notify: Default::default(),
            update: tx,
            killed: AtomicBool::new(false),
        }
    }
}

impl Rustpad {
    pub async fn load(document: PersistedDocument) -> Self {
        let mut operation = OperationSeq::default();
        operation.insert(&document.text);

        let rustpad = Self::default();
        {
            let mut state = rustpad.state.write().await;
            state.text = document.text;
            state.meta = document.meta;
            state.operations.push(UserOperation {
                id: u64::MAX,
                operation,
            })
        }
        rustpad
    }
    /// Handle a connection from a WebSocket.
    pub async fn on_connection(&self, mut socket: WebSocket, user: Option<UserInfo>) {
        let id = self.count.fetch_add(1, Ordering::Relaxed);
        info!("connection id={id}");
        if let Err(e) = self.handle_connection(id, &mut socket, user).await {
            warn!("connection terminated early: {e}");
            socket.close().await.ok();
        }
        socket.close().await.ok();
        info!("disconnection, id = {id}");
        {
            let mut state = self.state.write().await;
            state.users.remove(&id);
            state.cursors.remove(&id);
        }

        self.update
            .send(ServerMsg::UserInfo { id, info: None })
            .ok();
    }

    pub async fn is_limited(&self) -> bool {
        let state = self.state.read().await;
        state.meta.limited
    }

    /// Returns a snapshot of the current document for persistence.
    pub async fn snapshot(&self) -> PersistedDocument {
        let state = self.state.read().await;
        PersistedDocument {
            text: state.text.clone(),
            meta: state.meta.clone(),
        }
    }

    /// Returns the current revision.
    pub async fn revision(&self) -> usize {
        let state = self.state.read().await;
        state.operations.len()
    }

    // Returns the document if it has been modified since the last call to `dirty_snapshot`,
    // and resets the dirty flag.
    //
    // Has to be done as one operation to avoid "lost wakeup".
    pub async fn dirty_snapshot(&self) -> Option<PersistedDocument> {
        let mut state = self.state.write().await;
        if state.dirty {
            state.dirty = false;
            Some(PersistedDocument {
                text: state.text.clone(),
                meta: state.meta.clone(),
            })
        } else {
            None
        }
    }

    pub async fn kill_if_idle(&self) -> bool {
        let state = self.state.read().await;
        if state.users.is_empty() && !state.dirty {
            self.kill();
            true
        } else {
            false
        }
    }

    /// Kill this object immediately, dropping all current connections.
    pub fn kill(&self) {
        self.killed.store(true, Ordering::Relaxed);
        self.notify.notify_waiters();
    }

    /// Returns if this Rustpad object has been killed.
    pub fn killed(&self) -> bool {
        self.killed.load(Ordering::Relaxed)
    }

    async fn handle_connection(
        &self,
        id: u64,
        socket: &mut WebSocket,
        user: Option<UserInfo>,
    ) -> Result<()> {
        let mut update_rx = self.update.subscribe();

        let mut revision: usize = self.send_initial(id, socket, user.clone()).await?;
        let is_admin = user.as_ref().is_some_and(|u| u.admin);

        loop {
            // In order to avoid the "lost wakeup" problem, we first request a
            // notification, **then** check the current state for new revisions.
            // This is the same approach that `tokio::sync::watch` takes.
            let notified = self.notify.notified();
            if self.killed() {
                break;
            }
            if !is_admin && self.is_limited().await {
                info!("disconnecting non-admin user from closed document");
                break;
            }
            if self.revision().await > revision {
                revision = self.send_history(revision, socket).await?
            }

            tokio::select! {
                _ = notified => {}
                update = update_rx.recv() => {
                    socket.send(update?.into()).await?;
                }
                result = socket.next() => {
                    match result {
                        None => break,
                        Some(message) => {
                            self.handle_message(id, message?, &user).await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn send_initial(
        &self,
        id: u64,
        socket: &mut WebSocket,
        info: Option<UserInfo>,
    ) -> Result<usize> {
        socket.send(ServerMsg::Identity { id, info }.into()).await?;
        let mut messages = Vec::new();
        let revision = {
            let state = self.state.read().await;
            messages.push(ServerMsg::Meta {
                language: state.meta.language.clone(),
                limited: state.meta.limited,
            });
            if !state.operations.is_empty() {
                messages.push(ServerMsg::History {
                    start: 0,
                    operations: state.operations.clone(),
                });
            }
            for (&id, info) in &state.users {
                messages.push(ServerMsg::UserInfo {
                    id,
                    info: Some(info.clone()),
                });
            }
            for (&id, data) in &state.cursors {
                messages.push(ServerMsg::UserCursor {
                    id,
                    data: data.clone(),
                });
            }
            state.operations.len()
        };
        for msg in messages {
            socket.send(msg.into()).await?;
        }
        Ok(revision)
    }

    async fn send_history(&self, start: usize, socket: &mut WebSocket) -> Result<usize> {
        let operations = {
            let state = self.state.read().await;
            let len = state.operations.len();
            if start < len {
                state.operations[start..].to_owned()
            } else {
                Vec::new()
            }
        };
        let num_ops = operations.len();
        if num_ops > 0 {
            let msg = ServerMsg::History { start, operations };
            socket.send(msg.into()).await?;
        }
        Ok(start + num_ops)
    }

    async fn handle_message(
        &self,
        id: u64,
        message: Message,
        user: &Option<UserInfo>,
    ) -> Result<()> {
        let msg: ClientMsg = match message.to_text() {
            Ok(text) => serde_json::from_str(text).context("failed to deserialize message")?,
            Err(_) => return Ok(()), // Ignore non-text messages
        };
        match msg {
            ClientMsg::Edit {
                revision,
                operation,
            } => {
                self.apply_edit(id, revision, operation)
                    .await
                    .context("invalid edit operation")?;
                self.notify.notify_waiters();
            }
            ClientMsg::SetMeta { language, limited } => {
                let mut state = self.state.write().await;
                if let Some(language) = language.clone() {
                    state.meta.language = language;
                }
                let language = state.meta.language.clone();
                if let Some(limited) = limited {
                    state.meta.limited = limited;
                    if limited {
                        info!("document is now limited, disconnecting non-admin users");
                        self.notify.notify_waiters();
                    }
                }
                let limited = state.meta.limited;
                state.dirty = true;
                drop(state);
                self.update.send(ServerMsg::Meta { language, limited }).ok();
            }
            ClientMsg::ClientInfo(mut info) => {
                // Ensure clients can't lie about being admins
                if let Some(user) = user {
                    info.admin = user.admin;
                    if info.admin {
                        // Admins cannot change their name
                        info.name = user.name.clone();
                    }
                }
                info.hue %= 360;
                self.state.write().await.users.insert(id, info.clone());
                let msg = ServerMsg::UserInfo {
                    id,
                    info: Some(info),
                };
                self.update.send(msg).ok();
            }
            ClientMsg::CursorData(data) => {
                self.state.write().await.cursors.insert(id, data.clone());
                let msg = ServerMsg::UserCursor { id, data };
                self.update.send(msg).ok();
            }
        }
        Ok(())
    }

    async fn apply_edit(
        &self,
        id: u64,
        revision: usize,
        mut operation: OperationSeq,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let len = state.operations.len();
        if revision > len {
            bail!("got revision {}, but current is {}", revision, len);
        }
        for history_op in &state.operations[revision..] {
            operation = operation.transform(&history_op.operation)?.0;
        }
        if operation.target_len() > 256 * 1024 {
            bail!(
                "target length {} is greater than 256 KiB maximum",
                operation.target_len()
            );
        }
        let new_text = operation.apply(&state.text)?;
        for (_, data) in state.cursors.iter_mut() {
            for cursor in data.cursors.iter_mut() {
                *cursor = transform_index(&operation, *cursor);
            }
            for (start, end) in data.selections.iter_mut() {
                *start = transform_index(&operation, *start);
                *end = transform_index(&operation, *end);
            }
        }
        state.operations.push(UserOperation { id, operation });
        state.text = new_text;
        state.dirty = true;
        Ok(())
    }
}

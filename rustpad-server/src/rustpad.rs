//! Eventually consistent server-side logic for Rustpad.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use axum::extract::ws::Message;

use serde::{Deserialize, Serialize};
use tokio::sync::futures::Notified;
use tokio::sync::{Notify, RwLock, broadcast};
use tracing::info;

use crate::auth::User;
use crate::{
    database::PersistedDocument,
    delta::{DEFAULT_QUILL_EMBEDS, Delta},
};

const MAX_DOCUMENT_BYTES: usize = 256 * 1024;

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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Unauthenticated user.
    Anon,
    /// Authenticated user without admin privileges.
    User,
    /// Authenticated user with admin privileges.
    Admin,
}
impl Role {
    pub fn can_access(self, visibility: Visibility) -> bool {
        match visibility {
            Visibility::Private => self == Role::Admin,
            Visibility::Internal => self != Role::Anon,
            Visibility::Public => true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnlineUser {
    pub name: String,
    pub hue: u16,
    pub role: Role,
}
impl From<User> for OnlineUser {
    fn from(user: User) -> Self {
        Self {
            name: user.name,
            hue: user.hue,
            role: if user.admin { Role::Admin } else { Role::User },
        }
    }
}

/// Shared state involving multiple users, protected by a lock.
struct State {
    // TODO: track revisions per user and merge older operations
    operations: Vec<UserOperation>,
    document: Delta,
    meta: DocumentMeta,
    users: HashMap<u64, OnlineUser>,
    cursors: HashMap<u64, CursorData>,
    dirty: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            document: {
                let mut document = Delta::new();
                document
                    .insert("\n", None)
                    .expect("default document is valid");
                document
            },
            meta: DocumentMeta {
                visibility: Visibility::Public,
            },
            users: HashMap::new(),
            cursors: HashMap::new(),
            dirty: false,
        }
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
        let mut operation = document.delta.clone();
        operation
            .delete(1)
            .expect("persisted documents are validated when loaded");

        let rustpad = Self::default();
        {
            let mut state = rustpad.state.write().await;
            state.document = document.delta;
            state.meta = document.meta;
            state.operations.push(UserOperation {
                id: u64::MAX,
                operation,
            });
        }
        rustpad
    }

    pub async fn init_connection(&self, user: Option<User>) -> (u64, usize, Vec<ServerMsg>) {
        let id = self.count.fetch_add(1, Ordering::Relaxed);
        info!("initializing connection id={id}");

        let mut messages = Vec::new();
        messages.push(ServerMsg::Identity {
            id,
            info: user.map(|u| u.into()),
        });
        let state = self.state.read().await;
        messages.push(ServerMsg::Meta(state.meta.clone()));
        if !state.operations.is_empty() {
            messages.push(ServerMsg::History {
                start: 0,
                operations: state.operations.clone(),
            });
        }
        for (&id, info) in &state.users {
            messages.push(ServerMsg::UserInfo {
                id,
                user: info.clone(),
            });
        }
        for (&id, data) in &state.cursors {
            messages.push(ServerMsg::UserCursor {
                id,
                data: data.clone(),
            });
        }
        let revision = state.operations.len();
        (id, revision, messages)
    }

    pub async fn close_connection(&self, id: u64) {
        info!("disconnection, id = {id}");
        {
            let mut state = self.state.write().await;
            state.users.remove(&id);
            state.cursors.remove(&id);
        }

        self.update.send(ServerMsg::UserDisconnect { id }).ok();
    }

    pub fn notified(&self) -> Notified<'_> {
        self.notify.notified()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerMsg> {
        self.update.subscribe()
    }

    pub async fn visibility(&self) -> Visibility {
        let state = self.state.read().await;
        state.meta.visibility
    }

    /// Returns a snapshot of the current document for persistence.
    pub async fn snapshot(&self) -> PersistedDocument {
        let state = self.state.read().await;
        PersistedDocument {
            delta: state.document.clone(),
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
                delta: state.document.clone(),
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

    pub async fn send_history(&self, start: usize) -> Result<(usize, ServerMsg)> {
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
        Ok((start + num_ops, ServerMsg::History { start, operations }))
    }

    pub async fn update_user(&self, user: OnlineUser) {
        let mut state = self.state.write().await;
        for (&id, existing_user) in state.users.iter_mut() {
            if existing_user.name == user.name {
                *existing_user = user.clone();
                drop(state);
                self.update.send(ServerMsg::UserInfo { id, user }).ok();
                break;
            }
        }
    }

    pub async fn handle_message(
        &self,
        id: u64,
        message: ClientMsg,
        user: &Option<User>,
    ) -> Result<()> {
        match message {
            ClientMsg::Edit {
                revision,
                operation,
            } => {
                self.apply_edit(id, revision, operation)
                    .await
                    .context("invalid edit operation")?;
                self.notify.notify_waiters();
            }
            ClientMsg::SetMeta { visibility } => {
                let mut state = self.state.write().await;
                if let Some(visibility) = visibility {
                    let old_visibility = state.meta.visibility;
                    state.meta.visibility = visibility;
                    if visibility < old_visibility {
                        info!("document is now private, disconnecting non-admin users");
                        self.notify.notify_waiters();
                    }
                }
                let meta = state.meta.clone();
                state.dirty = true;
                drop(state);
                self.update.send(ServerMsg::Meta(meta)).ok();
            }
            ClientMsg::ClientInfo { name, hue } => {
                let mut new = OnlineUser {
                    name,
                    hue: hue % 360,
                    role: Role::Anon,
                };
                // Ensure clients can't lie
                if let Some(user) = user {
                    new.name = user.name.clone();
                    new.role = if user.admin { Role::Admin } else { Role::User };
                }
                self.state.write().await.users.insert(id, new.clone());
                let msg = ServerMsg::UserInfo { id, user: new };
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

    async fn apply_edit(&self, id: u64, revision: usize, mut operation: Delta) -> Result<()> {
        operation.validate_quill(DEFAULT_QUILL_EMBEDS)?;
        if serde_json::to_vec(&operation)?.len() > MAX_DOCUMENT_BYTES {
            bail!("edit operation is greater than 256 KiB maximum");
        }

        let mut state = self.state.write().await;
        let len = state.operations.len();
        if revision > len {
            bail!("got revision {}, but current is {}", revision, len);
        }
        for history_op in &state.operations[revision..] {
            operation = history_op.operation.transform(&operation, true)?;
        }
        operation.validate_quill(DEFAULT_QUILL_EMBEDS)?;
        let new_document = operation.apply_to_document(&state.document)?;
        if !new_document.ends_with_newline() {
            bail!("Quill documents must end with a newline");
        }
        let document_length = new_document.length()?;
        if document_length > MAX_DOCUMENT_BYTES
            || serde_json::to_vec(&new_document)?.len() > MAX_DOCUMENT_BYTES
        {
            bail!(
                "document length {} is greater than 256 KiB maximum",
                document_length
            );
        }
        for data in state.cursors.values_mut() {
            for cursor in &mut data.cursors {
                *cursor = u32::try_from(operation.transform_position(*cursor as usize, false)?)?;
            }
            for (start, end) in &mut data.selections {
                *start = u32::try_from(operation.transform_position(*start as usize, false)?)?;
                *end = u32::try_from(operation.transform_position(*end as usize, false)?)?;
            }
        }
        state.operations.push(UserOperation { id, operation });
        state.document = new_document;
        state.dirty = true;
        Ok(())
    }
}

/// Metadata for a persisted document.
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct DocumentMeta {
    /// If accessible by external users.
    pub visibility: Visibility,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Document is only accessible by admins.
    Private,
    /// Document is only accessible by authenticated users.
    Internal,
    /// Document is accessible by anyone with the link.
    Public,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserOperation {
    pub id: u64,
    pub operation: Delta,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CursorData {
    pub cursors: Vec<u32>,
    pub selections: Vec<(u32, u32)>,
}

/// A message received from the client over WebSocket.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClientMsg {
    /// Represents a sequence of local edits from the user.
    Edit { revision: usize, operation: Delta },
    /// Sets the metadata of the editor.
    SetMeta { visibility: Option<Visibility> },
    /// Sets the user's current information.
    ClientInfo { name: String, hue: u16 },
    /// Sets the user's cursor and selection positions.
    CursorData(CursorData),
}

/// A message sent to the client over WebSocket.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerMsg {
    /// Informs the client of their unique socket ID and admin status.
    Identity { id: u64, info: Option<OnlineUser> },
    /// Broadcasts text operations to all clients.
    History {
        start: usize,
        operations: Vec<UserOperation>,
    },
    /// Broadcasts the current metadata, last writer wins.
    Meta(DocumentMeta),
    /// Broadcasts a user's information.
    UserInfo { id: u64, user: OnlineUser },
    /// Broadcasts a user's disconnection.
    UserDisconnect { id: u64 },
    /// Broadcasts a user's cursor position.
    UserCursor { id: u64, data: CursorData },
}

impl From<ServerMsg> for Message {
    fn from(msg: ServerMsg) -> Self {
        let serialized = serde_json::to_string(&msg).expect("failed serialize");
        Message::text(serialized)
    }
}

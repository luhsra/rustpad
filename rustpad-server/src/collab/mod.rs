use std::sync::Arc;

use axum::extract::ws::WebSocket;
use futures::StreamExt;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

mod broadcast;
mod connection;
mod signaling;
mod websocket;

use websocket::{AxumSink, AxumStream};
use yrs::{AsyncTransact, Doc, GetString, Text, sync::Awareness};

use crate::Visibility;
use crate::collab::broadcast::BroadcastGroup;
use crate::database::PersistedDocument;

pub struct Document {
    bcast: broadcast::BroadcastGroup,
    state: Arc<RwLock<State>>,
}

struct State {
    visibility: Visibility,
}

impl Document {
    pub async fn new(content: String) -> Self {
        let awareness = {
            let doc = Doc::new();
            {
                let txt = doc.get_or_insert_text("srapad");
                let mut txn = doc.transact_mut().await;
                txt.push(&mut txn, &content);
            }
            Arc::new(Awareness::new(doc))
        };

        Self {
            bcast: BroadcastGroup::new(awareness.clone(), 32).await,
            state: Arc::new(RwLock::new(State {
                visibility: Visibility::Public,
            })),
        }
    }

    pub async fn snapshot(&self) -> PersistedDocument {
        let awareness = self.bcast.awareness();
        let doc = awareness.doc();
        let text = doc.get_or_insert_text("codemirror");
        let txn = doc.transact().await;
        let markdown = text.get_string(&txn);
        let state = self.state.read().await;
        PersistedDocument::new(markdown, state.visibility)
    }

    pub async fn dirty_snapshot(&self) -> Option<PersistedDocument> {
        // TODO: Return only if document has changed since last snapshot
        Some(self.snapshot().await)
    }

    pub async fn visibility(&self) -> Visibility {
        let state = self.state.read().await;
        state.visibility
    }

    pub async fn is_idle(&self) -> bool {
        // self.bcast.is_idle().await
        false
    }
}

pub async fn peer(ws: WebSocket, document: Arc<Document>) {
    let (sink, stream) = ws.split();
    let sink = Arc::new(Mutex::new(AxumSink::from(sink)));
    let stream = AxumStream::from(stream);
    let sub = document.bcast.subscribe(sink, stream);
    match sub.completed().await {
        Ok(_) => info!("broadcasting for channel finished successfully"),
        Err(e) => warn!("broadcasting for channel finished abruptly: {}", e),
    }
}

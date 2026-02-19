use std::sync::Arc;

use axum::extract::ws::WebSocket;
use futures::StreamExt;
use tokio::sync::Mutex;
use tracing::{info, warn};

mod broadcast;
mod connection;
mod signaling;
mod websocket;

use websocket::{AxumSink, AxumStream};
use yrs::{AsyncTransact, Doc, GetString, Text, sync::Awareness};

use crate::collab::broadcast::BroadcastGroup;

pub struct Document {
    bcast: broadcast::BroadcastGroup,
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

        let bcast = BroadcastGroup::new(awareness.clone(), 32).await;
        Self { bcast }
    }

    pub async fn snapshot(&self) -> String {
        let awareness = self.bcast.awareness();
        let doc = awareness.doc();
        let text = doc.get_or_insert_text("codemirror");
        let txn = doc.transact().await;
        text.get_string(&txn)
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

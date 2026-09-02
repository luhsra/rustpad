#![cfg(test)]
//! Tests to ensure that documents are garbage collected.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use common::*;

use rustpad_server::{ServerState, server};
use serde_json::json;
use tokio::time;

pub mod common;

#[ignore = "This is currently not supported"]
#[tokio::test]
async fn test_cleanup() -> Result<()> {
    logging();
    let app = server(Arc::new(ServerState::temporary().await?));
    let client = TestClient::start(app).await?;

    client.expect_text("old", "").await;

    let mut socket = client.connect("old").await?;
    let msg = socket.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 0, "info": () } }));
    assert!(socket.recv().await?.get("Meta").is_some());

    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": { "ops": [{ "insert": "hello" }] }
        }
    });
    socket.send(&msg).await;

    let msg = socket.recv().await?;
    msg.get("History")
        .expect("should receive history operation");
    client.expect_text("old", "hello").await;

    let hour = Duration::from_secs(3600);
    time::pause();
    time::advance(47 * hour).await;
    client.expect_text("old", "hello").await;

    time::advance(3 * hour).await;
    client.expect_text("old", "").await;

    Ok(())
}

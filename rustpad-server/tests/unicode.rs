//! Tests for Quill-compatible UTF-16 indexing and cursor transformation.

pub mod common;

use std::sync::Arc;

use anyhow::Result;
use common::*;
use rustpad_server::{ServerState, server};
use serde_json::json;

#[tokio::test]
async fn test_unicode_length() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;
    let mut socket = client.connect("unicode").await?;
    socket.recv().await?;
    socket.recv().await?;

    let text = "h🎉e🎉l👨‍👨‍👦‍👦lo";
    socket
        .send(&json!({
            "Edit": {
                "revision": 0,
                "operation": { "ops": [{ "insert": text }] }
            }
        }))
        .await;
    socket.recv().await?;
    client
        .expect_text("unicode", &(text.to_owned() + "\n"))
        .await;

    socket
        .send(&json!({
            "Edit": {
                "revision": 1,
                "operation": { "ops": [{ "delete": text.encode_utf16().count() }] }
            }
        }))
        .await;
    assert_eq!(
        socket.recv().await?,
        json!({
            "History": {
                "start": 1,
                "operations": [{
                    "id": 0,
                    "operation": { "ops": [{ "delete": text.encode_utf16().count() }] }
                }]
            }
        })
    );
    client.expect_text("unicode", "\n").await;
    Ok(())
}

#[tokio::test]
async fn test_unicode_cursors() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;
    let mut socket = client.connect("unicode").await?;
    socket.recv().await?;
    socket.recv().await?;

    socket
        .send(&json!({
            "Edit": {
                "revision": 0,
                "operation": { "ops": [{ "insert": "🎉🎉🎉" }] }
            }
        }))
        .await;
    socket.recv().await?;

    let cursors = json!({
        "cursors": [0, 2, 4, 6],
        "selections": [[0, 2], [4, 6]]
    });
    socket.send(&json!({ "CursorData": cursors })).await;
    socket.recv().await?;

    let mut socket2 = client.connect("unicode").await?;
    socket2.recv().await?;
    socket2.recv().await?;
    socket2.recv().await?;
    socket2.recv().await?;

    socket2
        .send(&json!({
            "Edit": {
                "revision": 1,
                "operation": { "ops": [{ "insert": "😍" }] }
            }
        }))
        .await;
    socket2.recv().await?;

    let mut socket3 = client.connect("unicode").await?;
    socket3.recv().await?;
    socket3.recv().await?;
    socket3.recv().await?;
    assert_eq!(
        socket3.recv().await?,
        json!({
            "UserCursor": {
                "id": 0,
                "data": {
                    "cursors": [2, 4, 6, 8],
                    "selections": [[2, 4], [6, 8]]
                }
            }
        })
    );
    Ok(())
}

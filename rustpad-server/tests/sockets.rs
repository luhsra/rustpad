//! Basic tests for real-time collaboration.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use common::*;
use rustpad_server::{ServerState, server};
use serde_json::json;
use tokio::time;

pub mod common;

#[tokio::test]
async fn test_single_operation() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    client.expect_text("foobar", "").await;

    let mut socket = client.connect("foobar").await?;
    assert_eq!(
        socket.recv().await?,
        json!({ "Identity": { "id": 0, "info": () } })
    );
    assert!(socket.recv().await?.get("Meta").is_some());

    socket
        .send(&json!({
            "Edit": {
                "revision": 0,
                "operation": { "ops": [{ "insert": "hello" }] }
            }
        }))
        .await;

    assert_eq!(
        socket.recv().await?,
        json!({
            "History": {
                "start": 0,
                "operations": [
                    { "id": 0, "operation": { "ops": [{ "insert": "hello" }] } }
                ]
            }
        })
    );

    client.expect_text("foobar", "hello\n").await;
    Ok(())
}

#[tokio::test]
async fn test_invalid_operation() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;
    let mut socket = client.connect("foobar").await?;
    socket.recv().await?;
    socket.recv().await?;

    socket
        .send(&json!({
            "Edit": {
                "revision": 1,
                "operation": { "ops": [{ "insert": "hello" }] }
            }
        }))
        .await;

    socket.recv_closed().await?;
    Ok(())
}

#[tokio::test]
async fn test_concurrent_transform() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    let mut socket = client.connect("foobar").await?;
    assert_eq!(
        socket.recv().await?,
        json!({ "Identity": { "id": 0, "info": () } })
    );
    assert!(socket.recv().await?.get("Meta").is_some());

    socket
        .send(&json!({
            "Edit": {
                "revision": 0,
                "operation": { "ops": [{ "insert": "hello" }] }
            }
        }))
        .await;
    assert_eq!(
        socket.recv().await?,
        json!({
            "History": {
                "start": 0,
                "operations": [
                    { "id": 0, "operation": { "ops": [{ "insert": "hello" }] } }
                ]
            }
        })
    );

    socket
        .send(&json!({
            "Edit": {
                "revision": 1,
                "operation": { "ops": [
                    { "retain": 2 }, { "insert": "n" }, { "delete": 1 }
                ] }
            }
        }))
        .await;
    assert_eq!(
        socket.recv().await?,
        json!({
            "History": {
                "start": 1,
                "operations": [
                    { "id": 0, "operation": { "ops": [
                        { "retain": 2 }, { "insert": "n" }, { "delete": 1 }
                    ] } }
                ]
            }
        })
    );
    client.expect_text("foobar", "henlo\n").await;

    let mut socket2 = client.connect("foobar").await?;
    assert_eq!(
        socket2.recv().await?,
        json!({ "Identity": { "id": 1, "info": () } })
    );
    assert!(socket2.recv().await?.get("Meta").is_some());

    time::sleep(Duration::from_millis(50)).await;
    socket2
        .send(&json!({
            "Edit": {
                "revision": 0,
                "operation": { "ops": [{ "insert": "~rust~" }] }
            }
        }))
        .await;

    assert_eq!(
        socket2.recv().await?,
        json!({
            "History": {
                "start": 0,
                "operations": [
                    { "id": 0, "operation": { "ops": [{ "insert": "hello" }] } },
                    { "id": 0, "operation": { "ops": [
                        { "retain": 2 }, { "insert": "n" }, { "delete": 1 }
                    ] } }
                ]
            }
        })
    );

    let transformed = json!({
        "History": {
            "start": 2,
            "operations": [
                { "id": 1, "operation": { "ops": [
                    { "retain": 5 }, { "insert": "~rust~" }
                ] } }
            ]
        }
    });
    assert_eq!(socket.recv().await?, transformed);
    assert_eq!(socket2.recv().await?, transformed);
    client.expect_text("foobar", "henlo~rust~\n").await;
    Ok(())
}

#[tokio::test]
async fn test_set_meta() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    let mut socket = client.connect("foobar").await?;
    socket.recv().await?;
    assert_eq!(
        socket.recv().await?,
        json!({ "Meta": { "visibility": "public" } })
    );

    socket
        .send(&json!({ "SetMeta": { "visibility": "public" } }))
        .await;
    assert_eq!(
        socket.recv().await?,
        json!({ "Meta": { "visibility": "public" } })
    );

    client.expect_text("foobar", "\n").await;
    Ok(())
}

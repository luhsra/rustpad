//! Basic tests for real-time collaboration.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use common::*;
use tracing::info;
use operational_transform::OperationSeq;
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
    let msg = socket.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 0, "info": () } }));
    assert!(socket.recv().await?.get("Meta").is_some());

    let mut operation = OperationSeq::default();
    operation.insert("hello");
    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": operation
        }
    });
    info!("sending ClientMsg {}", msg);
    socket.send(&msg).await;

    let msg = socket.recv().await?;
    assert_eq!(
        msg,
        json!({
            "History": {
                "start": 0,
                "operations": [
                    { "id": 0, "operation": ["hello"] }
                ]
            }
        })
    );

    client.expect_text("foobar", "hello").await;
    Ok(())
}

#[tokio::test]
async fn test_invalid_operation() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    client.expect_text("foobar", "").await;

    let mut socket = client.connect("foobar").await?;
    let msg = socket.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 0, "info": () } }));
    assert!(socket.recv().await?.get("Meta").is_some());

    let mut operation = OperationSeq::default();
    operation.insert("hello");
    let msg = json!({
        "Edit": {
            "revision": 1,
            "operation": operation
        }
    });
    info!("sending ClientMsg {}", msg);
    socket.send(&msg).await;

    socket.recv_closed().await?;
    Ok(())
}

#[tokio::test]
async fn test_concurrent_transform() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    // Connect the first client
    let mut socket = client.connect("foobar").await?;
    let msg = socket.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 0, "info": () } }));
    assert!(socket.recv().await?.get("Meta").is_some());

    // Insert the first operation
    let mut operation = OperationSeq::default();
    operation.insert("hello");
    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": operation
        }
    });
    info!("sending ClientMsg {}", msg);
    socket.send(&msg).await;

    let msg = socket.recv().await?;
    assert_eq!(
        msg,
        json!({
            "History": {
                "start": 0,
                "operations": [
                    { "id": 0, "operation": ["hello"] }
                ]
            }
        })
    );

    // Insert the second operation
    let mut operation = OperationSeq::default();
    operation.retain(2);
    operation.delete(1);
    operation.insert("n");
    operation.retain(2);
    let msg = json!({
        "Edit": {
            "revision": 1,
            "operation": operation
        }
    });
    info!("sending ClientMsg {}", msg);
    socket.send(&msg).await;

    let msg = socket.recv().await?;
    assert_eq!(
        msg,
        json!({
            "History": {
                "start": 1,
                "operations": [
                    { "id": 0, "operation": [2, "n", -1, 2] }
                ]
            }
        })
    );
    client.expect_text("foobar", "henlo").await;

    // Connect the second client
    let mut socket2 = client.connect("foobar").await?;
    let msg = socket2.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 1, "info": () } }));
    assert!(socket2.recv().await?.get("Meta").is_some(), "{msg}");

    // Insert a concurrent operation before seeing the existing history
    time::sleep(Duration::from_millis(50)).await;
    let mut operation = OperationSeq::default();
    operation.insert("~rust~");
    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": operation
        }
    });
    info!("sending ClientMsg {}", msg);
    socket2.send(&msg).await;

    // Receive the existing history
    let msg = socket2.recv().await?;
    assert_eq!(
        msg,
        json!({
            "History": {
                "start": 0,
                "operations": [
                    { "id": 0, "operation": ["hello"] },
                    { "id": 0, "operation": [2, "n", -1, 2] }
                ]
            }
        })
    );

    // Expect to receive a transformed operation
    let transformed_op = json!({
        "History": {
            "start": 2,
            "operations": [
                { "id": 1, "operation": ["~rust~", 5] },
            ]
        }
    });

    // ... in the first client
    let msg = socket.recv().await?;
    assert_eq!(msg, transformed_op);

    // ... and in the second client
    let msg = socket2.recv().await?;
    assert_eq!(msg, transformed_op);

    client.expect_text("foobar", "~rust~henlo").await;
    Ok(())
}

#[tokio::test]
async fn test_set_meta() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    let mut socket = client.connect("foobar").await?;
    let msg = socket.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 0, "info": () } }));
    assert!(socket.recv().await?.get("Meta").is_some());

    let msg = json!({ "SetMeta": { "language": "javascript", "limited": false } });
    socket.send(&msg).await;

    let msg = socket.recv().await?;
    assert_eq!(
        msg,
        json!({ "Meta": { "language": "javascript", "limited": false } })
    );

    let mut socket2 = client.connect("foobar").await?;
    let msg = socket2.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 1, "info": () } }));
    let msg = socket2.recv().await?;
    assert_eq!(
        msg,
        json!({ "Meta": { "language": "javascript", "limited": false } })
    );

    let msg = json!({ "SetMeta": { "language": "python", "limited": false } });
    socket2.send(&msg).await;

    let msg = socket.recv().await?;
    assert_eq!(
        msg,
        json!({ "Meta": { "language": "python", "limited": false } })
    );
    let msg = socket2.recv().await?;
    assert_eq!(
        msg,
        json!({ "Meta": { "language": "python", "limited": false } })
    );

    client.expect_text("foobar", "").await;
    Ok(())
}

//! Stress tests for liveness and consistency properties.

use std::{sync::Arc, time::Duration};

use anyhow::{Result, anyhow};
use common::*;
use tracing::info;
use operational_transform::OperationSeq;
use rustpad_server::{ServerState, server};
use serde_json::{Value, json};
use tokio::time::Instant;

pub mod common;

#[tokio::test]
async fn test_lost_wakeups() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    client.expect_text("stress", "").await;

    let mut socket = client.connect("stress").await?;
    let msg = socket.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 0, "info": () } }));
    assert!(socket.recv().await?.get("Meta").is_some());

    let mut socket2 = client.connect("stress").await?;
    let msg = socket2.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 1, "info": () } }));
    assert!(socket2.recv().await?.get("Meta").is_some());

    let mut revision = 0;
    for i in 0..100 {
        let num_edits = i % 5 + 1;
        for _ in 0..num_edits {
            let mut operation = OperationSeq::default();
            operation.retain(revision);
            operation.insert("a");
            let msg = json!({
                "Edit": {
                    "revision": revision,
                    "operation": operation
                }
            });
            socket.send(&msg).await;
            revision += 1;
        }

        let start = Instant::now();

        let num_ops = |msg: &Value| -> Option<usize> {
            Some(msg.get("History")?.get("operations")?.as_array()?.len())
        };

        let mut total = 0;
        while total < num_edits {
            let msg = socket.recv().await?;
            total += num_ops(&msg).ok_or_else(|| anyhow!("missing json key"))?;
        }

        let mut total2 = 0;
        while total2 < num_edits {
            let msg = socket2.recv().await?;
            total2 += num_ops(&msg).ok_or_else(|| anyhow!("missing json key"))?;
        }

        info!("took {} ms", start.elapsed().as_millis());
        assert!(start.elapsed() <= Duration::from_millis(200));
    }

    client
        .expect_text("stress", &"a".repeat(revision as usize))
        .await;

    Ok(())
}

#[tokio::test]
async fn test_large_document() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    client.expect_text("stress", "").await;

    let mut socket = client.connect("stress").await?;
    let msg = socket.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 0, "info": () } }));
    assert!(socket.recv().await?.get("Meta").is_some());

    let mut operation = OperationSeq::default();
    operation.insert(&"a".repeat(5000));
    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": operation
        }
    });
    socket.send(&msg).await;
    socket.recv().await?;

    let mut operation = OperationSeq::default();
    operation.insert(&"a".repeat(500000));
    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": operation
        }
    });
    socket.send(&msg).await;
    socket.recv_closed().await?;

    Ok(())
}

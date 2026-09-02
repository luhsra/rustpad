//! Tests to ensure that Delta documents are persisted.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use common::*;
use rustpad_server::{
    ServerState,
    database::{Database, PersistedDocument},
    delta::Delta,
    rustpad::Visibility,
    server,
};
use serde_json::json;
use tokio::time;

pub mod common;

fn document(text: &str) -> Delta {
    let mut delta = Delta::new();
    delta.insert(text, None).unwrap();
    delta
}

#[tokio::test]
async fn test_database() -> Result<()> {
    logging();
    let database = Database::temporary().await?;
    let hello = "hello".parse().unwrap();
    let world = "world".parse().unwrap();
    assert!(database.load_document(&hello).await.is_err());

    let doc1 = PersistedDocument::new(document("Hello Text\n"), Visibility::Public);
    database.store_document(&hello, &doc1).await?;
    assert_eq!(database.load_document(&hello).await?, doc1);
    assert!(database.load_document(&world).await.is_err());

    let mut formatted = Delta::new();
    formatted.insert(
        "World",
        Some(serde_json::from_value(json!({ "bold": true }))?),
    )?;
    formatted.insert("\n", None)?;
    let doc2 = PersistedDocument::new(formatted, Visibility::Public);
    database.store_document(&world, &doc2).await?;
    assert_eq!(database.load_document(&hello).await?, doc1);
    assert_eq!(database.load_document(&world).await?, doc2);

    database.store_document(&hello, &doc2).await?;
    assert_eq!(database.load_document(&hello).await?, doc2);
    Ok(())
}

#[tokio::test]
async fn test_persist() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;
    client.expect_text("persist", "").await;

    let mut socket = client.connect("persist").await?;
    socket.recv().await?;
    assert!(socket.recv().await?.get("Meta").is_some());
    socket
        .send(&json!({
            "Edit": {
                "revision": 0,
                "operation": { "ops": [{ "insert": "hello", "attributes": { "bold": true } }] }
            }
        }))
        .await;
    socket.recv().await?;
    client.expect_text("persist", "hello\n").await;

    let hour = Duration::from_secs(3600);
    time::pause();
    time::advance(47 * hour).await;
    client.expect_text("persist", "hello\n").await;

    time::resume();
    time::sleep(Duration::from_millis(150)).await;
    time::pause();
    time::advance(3 * hour).await;
    client.expect_text("persist", "hello\n").await;
    Ok(())
}

//! Tests to ensure that documents are persisted with SQLite.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use common::*;
use operational_transform::OperationSeq;
use rustpad_server::{
    ServerState,
    database::{Database, PersistedDocument},
    server,
};
use serde_json::json;
use tokio::time;

pub mod common;

#[tokio::test]
async fn test_database() -> Result<()> {
    logging();

    let database = Database::temporary().await?;

    let hello = "hello".parse().unwrap();
    let world = "world".parse().unwrap();
    assert!(database.load_document(&hello).await.is_err());
    assert!(database.load_document(&world).await.is_err());

    let doc1 = PersistedDocument::new("Hello Text".into(), "markdown".into(), false);

    assert!(database.store_document(&hello, &doc1).await.is_ok());
    assert_eq!(database.load_document(&hello).await?, doc1);
    assert!(database.load_document(&world).await.is_err());

    let doc2 = PersistedDocument::new("print('World Text :)')".into(), "python".into(), false);

    assert!(database.store_document(&world, &doc2).await.is_ok());
    assert_eq!(database.load_document(&hello).await?, doc1);
    assert_eq!(database.load_document(&world).await?, doc2);

    assert!(database.store_document(&hello, &doc2).await.is_ok());
    assert_eq!(database.load_document(&hello).await?, doc2);

    Ok(())
}

#[tokio::test]
async fn test_persist() -> Result<()> {
    logging();

    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    client.expect_text("persist", "").await;

    let mut socket = client.connect("persist").await?;
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
    socket.send(&msg).await;

    let msg = socket.recv().await?;
    msg.get("History")
        .expect("should receive history operation");
    client.expect_text("persist", "hello").await;

    let hour = Duration::from_secs(3600);
    time::pause();
    time::advance(47 * hour).await;
    client.expect_text("persist", "hello").await;

    // Give SQLite some time to actually update the database.
    time::resume();
    time::sleep(Duration::from_millis(150)).await;
    time::pause();

    time::advance(3 * hour).await;
    client.expect_text("persist", "hello").await;

    Ok(())
}

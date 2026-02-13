//! Tests to ensure that documents are persisted with SQLite.

use std::time::Duration;

use anyhow::Result;
use common::*;
use operational_transform::OperationSeq;
use rustpad_server::{
    ServerConfig,
    database::{Database, PersistedDocument},
    server,
};
use serde_json::json;
use tokio::time;

pub mod common;

#[tokio::test]
async fn test_database() -> Result<()> {
    pretty_env_logger::try_init().ok();

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
    pretty_env_logger::try_init().ok();

    let filter = server(ServerConfig {
        expiry_days: 2,
        database: Database::temporary().await?,
    });

    expect_text(&filter, "persist", "").await;

    let mut client = connect(&filter, "persist").await?;
    let msg = client.recv().await?;
    assert_eq!(msg, json!({ "Identity": 0 }));
    assert!(client.recv().await?.get("Meta").is_some());

    let mut operation = OperationSeq::default();
    operation.insert("hello");
    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": operation
        }
    });
    client.send(&msg).await;

    let msg = client.recv().await?;
    msg.get("History")
        .expect("should receive history operation");
    expect_text(&filter, "persist", "hello").await;

    let hour = Duration::from_secs(3600);
    time::pause();
    time::advance(47 * hour).await;
    expect_text(&filter, "persist", "hello").await;

    // Give SQLite some time to actually update the database.
    time::resume();
    time::sleep(Duration::from_millis(150)).await;
    time::pause();

    time::advance(3 * hour).await;
    expect_text(&filter, "persist", "hello").await;

    Ok(())
}

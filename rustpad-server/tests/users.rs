//! Tests for synchronization of user presence.

use std::sync::Arc;

use anyhow::Result;
use common::*;
use rustpad_server::{ServerState, server};
use serde_json::json;

pub mod common;

#[tokio::test]
async fn test_two_users() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    let mut socket = client.connect("foobar").await?;
    assert_eq!(
        socket.recv().await?,
        json!({ "Identity": { "id": 0, "info": () } })
    );
    assert!(socket.recv().await?.get("Meta").is_some());

    let alice = json!({
        "name": "Alice",
        "hue": 42,
        "admin": false,
    });
    socket.send(&json!({ "ClientInfo": alice })).await;

    let alice_info = json!({
        "UserInfo": {
            "id": 0,
            "info": alice,
        }
    });
    assert_eq!(socket.recv().await?, alice_info);

    let mut socket2 = client.connect("foobar").await?;
    assert_eq!(
        socket2.recv().await?,
        json!({ "Identity": { "id": 1, "info": () } })
    );
    assert!(socket2.recv().await?.get("Meta").is_some());
    assert_eq!(socket2.recv().await?, alice_info);

    let bob = json!({
        "name": "Bob",
        "hue": 96,
        "admin": false,
    });
    socket2.send(&json!({ "ClientInfo": bob })).await;

    let bob_info = json!({
        "UserInfo": {
            "id": 1,
            "info": bob,
        }
    });
    assert_eq!(socket2.recv().await?, bob_info);
    assert_eq!(socket.recv().await?, bob_info);

    Ok(())
}

#[tokio::test]
async fn test_invalid_user() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    let mut socket = client.connect("foobar").await?;
    assert_eq!(
        socket.recv().await?,
        json!({ "Identity": { "id": 0, "info": () } })
    );
    assert!(socket.recv().await?.get("Meta").is_some());

    let alice = json!({ "name": "Alice" }); // no hue
    socket.send(&json!({ "ClientInfo": alice })).await;
    socket.recv_closed().await?;

    Ok(())
}

#[tokio::test]
async fn test_leave_rejoin() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    let mut socket = client.connect("foobar").await?;
    assert_eq!(
        socket.recv().await?,
        json!({ "Identity": { "id": 0, "info": () } })
    );
    assert!(socket.recv().await?.get("Meta").is_some());

    let alice = json!({
        "name": "Alice",
        "hue": 42,
        "admin": false,
    });
    socket.send(&json!({ "ClientInfo": alice })).await;

    let alice_info = json!({
        "UserInfo": {
            "id": 0,
            "info": alice,
        }
    });
    assert_eq!(socket.recv().await?, alice_info);

    socket.send(&json!({ "Invalid": "please close" })).await;
    socket.recv_closed().await?;

    let mut socket2 = client.connect("foobar").await?;
    assert_eq!(
        socket2.recv().await?,
        json!({ "Identity": { "id": 1, "info": () } })
    );
    assert!(socket2.recv().await?.get("Meta").is_some());

    let bob = json!({
        "name": "Bob",
        "hue": 96,
        "admin": false,
    });
    socket2.send(&json!({ "ClientInfo": bob })).await;

    let bob_info = json!({
        "UserInfo": {
            "id": 1,
            "info": bob,
        }
    });
    assert_eq!(socket2.recv().await?, bob_info);

    Ok(())
}

#[tokio::test]
async fn test_cursors() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    let mut socket = client.connect("foobar").await?;
    assert_eq!(
        socket.recv().await?,
        json!({ "Identity": { "id": 0, "info": () } })
    );
    assert!(socket.recv().await?.get("Meta").is_some());

    let cursors = json!({
        "cursors": [4, 6, 7],
        "selections": [[5, 10], [3, 4]]
    });
    socket.send(&json!({ "CursorData": cursors })).await;

    let cursors_resp = json!({
        "UserCursor": {
            "id": 0,
            "data": cursors
        }
    });
    assert_eq!(socket.recv().await?, cursors_resp);

    let mut socket2 = client.connect("foobar").await?;
    assert_eq!(
        socket2.recv().await?,
        json!({ "Identity": { "id": 1, "info": () } })
    );
    assert!(socket2.recv().await?.get("Meta").is_some());
    assert_eq!(socket2.recv().await?, cursors_resp);

    let cursors2 = json!({
        "cursors": [10],
        "selections": []
    });
    socket2.send(&json!({ "CursorData": cursors2 })).await;

    let cursors2_resp = json!({
        "UserCursor": {
            "id": 1,
            "data": cursors2
        }
    });
    assert_eq!(socket2.recv().await?, cursors2_resp);
    assert_eq!(socket.recv().await?, cursors2_resp);

    socket.send(&json!({ "Invalid": "please close" })).await;
    socket.recv_closed().await?;

    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": ["a"]
        }
    });
    socket2.send(&msg).await;

    let mut socket3 = client.connect("foobar").await?;
    assert_eq!(
        socket3.recv().await?,
        json!({ "Identity": { "id": 2, "info": () } })
    );
    assert!(socket3.recv().await?.get("Meta").is_some());
    socket3.recv().await?;

    let transformed_cursors2_resp = json!({
        "UserCursor": {
            "id": 1,
            "data": {
                "cursors": [11],
                "selections": []
            }
        }
    });
    assert_eq!(socket3.recv().await?, transformed_cursors2_resp);

    Ok(())
}

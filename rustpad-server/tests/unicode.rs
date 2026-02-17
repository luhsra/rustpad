//! Tests for Unicode support and correct cursor transformation.

pub mod common;

use std::sync::Arc;

use anyhow::Result;
use common::*;
use operational_transform::OperationSeq;
use rustpad_server::{ServerState, server};
use serde_json::json;
use tracing::info;

#[tokio::test]
async fn test_unicode_length() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    client.expect_text("unicode", "").await;

    let mut socket = client.connect("unicode").await?;
    let msg = socket.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 0, "info": () } }));
    assert!(socket.recv().await?.get("Meta").is_some());

    let mut operation = OperationSeq::default();
    operation.insert("h🎉e🎉l👨‍👨‍👦‍👦lo");
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
                    { "id": 0, "operation": ["h🎉e🎉l👨‍👨‍👦‍👦lo"] }
                ]
            }
        })
    );

    info!("testing that text length is equal to number of Unicode code points...");
    let mut operation = OperationSeq::default();
    operation.delete(14);
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
                    { "id": 0, "operation": [-14] }
                ]
            }
        })
    );

    client.expect_text("unicode", "").await;

    Ok(())
}

#[tokio::test]
async fn test_multiple_operations() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    client.expect_text("unicode", "").await;

    let mut socket = client.connect("unicode").await?;
    let msg = socket.recv().await?;
    assert_eq!(msg, json!({ "Identity": { "id": 0, "info": () } }));
    assert!(socket.recv().await?.get("Meta").is_some());

    let mut operation = OperationSeq::default();
    operation.insert("🎉😍𒀇👨‍👨‍👦‍👦"); // Emoticons and Cuneiform
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
                    { "id": 0, "operation": ["🎉😍𒀇👨‍👨‍👦‍👦"] }
                ]
            }
        })
    );

    let mut operation = OperationSeq::default();
    operation.insert("👯‍♂️");
    operation.retain(3);
    operation.insert("𐅣𐅤𐅥"); // Ancient Greek numbers
    operation.retain(7);
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
                    { "id": 0, "operation": ["👯‍♂️", 3, "𐅣𐅤𐅥", 7] }
                ]
            }
        })
    );

    client.expect_text("unicode", "👯‍♂️🎉😍𒀇𐅣𐅤𐅥👨‍👨‍👦‍👦").await;

    let mut operation = OperationSeq::default();
    operation.retain(2);
    operation.insert("h̷̙̤̏͊̑̍̆̃̉͝ĕ̶̠̌̓̃̓̽̃̚l̸̥̊̓̓͝͠l̸̨̠̣̟̥͠ỏ̴̳̖̪̟̱̰̥̞̙̏̓́͗̽̀̈́͛͐̚̕͝͝ ̶̡͍͙͚̞͙̣̘͙̯͇̙̠̀w̷̨̨̪͚̤͙͖̝͕̜̭̯̝̋̋̿̿̀̾͛̐̏͘͘̕͝ǒ̴̙͉͈̗̖͍̘̥̤̒̈́̒͠r̶̨̡̢̦͔̙̮̦͖͔̩͈̗̖̂̀l̶̡̢͚̬̤͕̜̀͛̌̈́̈́͑͋̈̍̇͊͝͠ď̵̛̛̯͕̭̩͖̝̙͎̊̏̈́̎͊̐̏͊̕͜͝͠͝"); // Lots of ligatures
    operation.retain(8);
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
                "start": 2,
                "operations": [
                    { "id": 0, "operation": [6, "h̷̙̤̏͊̑̍̆̃̉͝ĕ̶̠̌̓̃̓̽̃̚l̸̥̊̓̓͝͠l̸̨̠̣̟̥͠ỏ̴̳̖̪̟̱̰̥̞̙̏̓́͗̽̀̈́͛͐̚̕͝͝ ̶̡͍͙͚̞͙̣̘͙̯͇̙̠̀w̷̨̨̪͚̤͙͖̝͕̜̭̯̝̋̋̿̿̀̾͛̐̏͘͘̕͝ǒ̴̙͉͈̗̖͍̘̥̤̒̈́̒͠r̶̨̡̢̦͔̙̮̦͖͔̩͈̗̖̂̀l̶̡̢͚̬̤͕̜̀͛̌̈́̈́͑͋̈̍̇͊͝͠ď̵̛̛̯͕̭̩͖̝̙͎̊̏̈́̎͊̐̏͊̕͜͝͠͝", 11] }
                ]
            }
        })
    );

    client
        .expect_text("unicode", "👯‍♂️🎉😍h̷̙̤̏͊̑̍̆̃̉͝ĕ̶̠̌̓̃̓̽̃̚l̸̥̊̓̓͝͠l̸̨̠̣̟̥͠ỏ̴̳̖̪̟̱̰̥̞̙̏̓́͗̽̀̈́͛͐̚̕͝͝ ̶̡͍͙͚̞͙̣̘͙̯͇̙̠̀w̷̨̨̪͚̤͙͖̝͕̜̭̯̝̋̋̿̿̀̾͛̐̏͘͘̕͝ǒ̴̙͉͈̗̖͍̘̥̤̒̈́̒͠r̶̨̡̢̦͔̙̮̦͖͔̩͈̗̖̂̀l̶̡̢͚̬̤͕̜̀͛̌̈́̈́͑͋̈̍̇͊͝͠ď̵̛̛̯͕̭̩͖̝̙͎̊̏̈́̎͊̐̏͊̕͜͝͠͝𒀇𐅣𐅤𐅥👨‍👨‍👦‍👦")
        .await;

    Ok(())
}

#[tokio::test]
async fn test_unicode_cursors() -> Result<()> {
    logging();
    let client = TestClient::start(server(Arc::new(ServerState::temporary().await?))).await?;

    let mut socket = client.connect("unicode").await?;
    assert_eq!(
        socket.recv().await?,
        json!({ "Identity": { "id": 0, "info": () } })
    );
    assert!(socket.recv().await?.get("Meta").is_some());

    let mut operation = OperationSeq::default();
    operation.insert("🎉🎉🎉");
    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": operation
        }
    });
    info!("sending ClientMsg {}", msg);
    socket.send(&msg).await;
    socket.recv().await?;

    let cursors = json!({
        "cursors": [0, 1, 2, 3],
        "selections": [[0, 1], [2, 3]]
    });
    socket.send(&json!({ "CursorData": cursors })).await;

    let cursors_resp = json!({
        "UserCursor": {
            "id": 0,
            "data": cursors
        }
    });
    assert_eq!(socket.recv().await?, cursors_resp);

    let mut socket2 = client.connect("unicode").await?;
    assert_eq!(
        socket2.recv().await?,
        json!({ "Identity": { "id": 1, "info": () } })
    );
    assert!(socket2.recv().await?.get("Meta").is_some());
    socket2.recv().await?;
    assert_eq!(socket2.recv().await?, cursors_resp);

    let msg = json!({
        "Edit": {
            "revision": 0,
            "operation": ["🎉"]
        }
    });
    socket2.send(&msg).await;

    let mut socket3 = client.connect("unicode").await?;
    assert_eq!(
        socket3.recv().await?,
        json!({ "Identity": { "id": 2, "info": () } })
    );
    assert!(socket3.recv().await?.get("Meta").is_some());
    socket3.recv().await?;

    let transformed_cursors_resp = json!({
        "UserCursor": {
            "id": 0,
            "data": {
                "cursors": [1, 2, 3, 4],
                "selections": [[1, 2], [3, 4]]
            }
        }
    });
    assert_eq!(socket3.recv().await?, transformed_cursors_resp);

    Ok(())
}

use std::net::SocketAddr;

use anyhow::{Result, anyhow};
use axum::http::StatusCode;
use futures::{SinkExt, StreamExt};
use tracing::info;
use openidconnect::reqwest;
use serde_json::Value;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// A test WebSocket client that sends and receives JSON messages.
pub struct JsonSocket(WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>);

impl JsonSocket {
    pub async fn send(&mut self, msg: &Value) {
        self.0.send(msg.to_string().into()).await.unwrap();
    }

    pub async fn recv(&mut self) -> Result<Value> {
        let msg = self
            .0
            .next()
            .await
            .ok_or_else(|| anyhow!("WebSocket closed"))??;
        let msg = msg.to_text().map_err(|_| anyhow!("non-string message"))?;
        Ok(serde_json::from_str(msg)?)
    }

    pub async fn recv_closed(&mut self) -> Result<()> {
        if let Some(Ok(Message::Close(_))) = self.0.next().await {
            Ok(())
        } else {
            Err(anyhow!("WebSocket should be closed"))
        }
    }
}

pub struct TestClient {
    client: reqwest::Client,
    addr: SocketAddr,
}
impl TestClient {
    pub async fn start(router: axum::Router) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0)))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, router.layer(TraceLayer::new_for_http())).into_future());
        let client = reqwest::Client::new();
        Ok(Self { client, addr })
    }
    pub async fn get(&self, path: &str) -> Result<String> {
        let url = format!("http://{}/{}", self.addr, path);
        info!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        assert_eq!(resp.status(), StatusCode::OK);
        Ok(resp.text().await?)
    }
    pub async fn expect_text(&self, id: &str, expected: &str) {
        let actual = self.get(&format!("api/text/{id}")).await.unwrap();
        assert_eq!(actual, expected);
    }
    pub async fn connect(&self, id: &str) -> Result<JsonSocket> {
        let (socket, _response) =
            tokio_tungstenite::connect_async(format!("ws://{}/api/socket/{id}", self.addr))
                .await
                .unwrap();
        Ok(JsonSocket(socket))
    }
}

pub fn logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=debug,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer().without_time())
        .try_init()
        .ok();
}

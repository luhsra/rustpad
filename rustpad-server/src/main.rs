use clap::Parser;
use rustpad_server::{ServerState, server};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
struct Args {
    #[clap(long, default_value = "0.0.0.0:3030")]
    host: SocketAddr,
    #[clap(short, long)]
    auth: Option<PathBuf>,
    #[clap(short, long, default_value = "storage")]
    storage: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=info,tower_http=info", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let config = Arc::new(
        ServerState::new(
            args.storage,
            args.auth.map(|path| {
                serde_json::from_str(&std::fs::read_to_string(path).expect("Opening auth config"))
                    .expect("Parsing auth config")
            }),
        )
        .await
        .expect("Init server state"),
    );

    info!("Starting server on http://{}", args.host);

    let listener = tokio::net::TcpListener::bind(args.host)
        .await
        .expect("Unable to bind to host");
    axum::serve(
        listener,
        server(config.clone()).layer(TraceLayer::new_for_http()),
    )
    // Yes we actually want to persist documents on shutdown...
    .with_graceful_shutdown(async { tokio::signal::ctrl_c().await.expect("Listen to ctrlc") })
    .await
    .unwrap();

    info!("Server has shut down");
    config.persist().await;
}

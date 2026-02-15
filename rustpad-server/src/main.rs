use rustpad_server::{ServerConfig, server};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let host: SocketAddr = std::env::var("HOST")
        .unwrap_or_else(|_| String::from("0.0.0.0:3030"))
        .parse()
        .expect("Unable to parse HOST");
    let config = ServerConfig::from_env()
        .await
        .expect("Unable to load server configuration");

    warp::serve(server(config))
        .bind(host)
        .await
        // Yes we actually want to persist documents on shutdown...
        .graceful(async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to listen to ctrlc")
        })
        .run()
        .await;
}

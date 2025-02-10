// src/main.rs

mod db;
mod document_symbols;
mod server;
mod workspace_symbols;

#[tokio::main]
async fn main() {
    env_logger::init();
    server::run().await;
}

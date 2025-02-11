// src/main.rs

mod db;
mod document_symbols;
mod goto_definition;
mod hover_preview;
mod link_references;
mod server;
mod workspace_symbols;

#[tokio::main]
async fn main() {
    env_logger::init();
    server::run().await;
}

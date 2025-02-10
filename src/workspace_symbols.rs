// src/workspace_symbols.rs

use futures::future::join_all;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tower_lsp::lsp_types::{
    Location, Position, Range, SymbolInformation, SymbolKind, WorkspaceSymbolParams,
};
use url::Url;

use crate::db;

/// Asynchronously gathers workspace symbols from all files stored in the database.
/// It uses the local path (the `path` field) rather than the virtual path.
/// If a query string is provided, the symbols are filtered (case‑insensitive).
pub async fn get_workspace_symbols(db: Arc<db::Database>, query: &str) -> Vec<SymbolInformation> {
    let mut all_symbols = Vec::new();

    // Get all file infos from the DB.
    let infos = match db.get_all_file_infos().await {
        Ok(infos) => infos,
        Err(e) => {
            log::error!("Error retrieving file infos from DB: {}", e);
            return all_symbols;
        }
    };

    // For each file info, we will:
    //   1. Use the local path stored in `info.path`.
    //   2. Convert it to a URI.
    //   3. Read the file content asynchronously.
    //   4. Extract markdown headings as symbols.
    let tasks = infos.into_iter().map(|info| {
        async move {
            let mut symbols = Vec::new();

            // Use the local path (not the virtual path).
            let local_path = info.path.clone();
            let path_buf = PathBuf::from(&local_path);
            let uri = match Url::from_file_path(&path_buf) {
                Ok(u) => u,
                Err(_) => {
                    log::error!("Could not convert local path {} to URI", local_path);
                    return symbols;
                }
            };

            // Read file contents asynchronously.
            let content = fs::read_to_string(&local_path).await.ok();

            if let Some(content) = content {
                // Extract markdown headings (lines starting with '#').
                let headings = extract_headings(&content);
                for (line, heading_text) in headings {
                    let symbol = SymbolInformation {
                        name: heading_text,
                        // You might adjust the SymbolKind based on your needs.
                        kind: SymbolKind::STRING,
                        location: Location {
                            uri: uri.clone(),
                            range: Range {
                                start: Position {
                                    line: line as u32,
                                    character: 0,
                                },
                                end: Position {
                                    line: line as u32,
                                    character: 0,
                                },
                            },
                        },
                        container_name: Some(info.title.clone()),
                        deprecated: None,
                        tags: None,
                    };
                    symbols.push(symbol);
                }
            } else {
                // If the file cannot be read, create a fallback symbol at the file level.
                symbols.push(SymbolInformation {
                    name: info.title.clone(),
                    kind: SymbolKind::FILE,
                    location: Location {
                        uri: uri.clone(),
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 0,
                            },
                            end: Position {
                                line: 0,
                                character: 0,
                            },
                        },
                    },
                    container_name: None,
                    deprecated: None,
                    tags: None,
                });
            }

            symbols
        }
    });

    // Run all tasks concurrently.
    let symbols_nested: Vec<Vec<SymbolInformation>> = join_all(tasks).await;
    all_symbols = symbols_nested.into_iter().flatten().collect();

    // If a query is provided, filter the symbols (case‑insensitive search).
    if !query.is_empty() {
        let query_lower = query.to_lowercase();
        all_symbols.retain(|sym| sym.name.to_lowercase().contains(&query_lower));
    }

    all_symbols
}

/// A simple helper that extracts markdown headings from the file content.
/// It returns a vector of tuples: (line number, heading text).
fn extract_headings(content: &str) -> Vec<(usize, String)> {
    let mut headings = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            // Remove all leading '#' characters and any extra whitespace.
            let heading_text = trimmed.trim_start_matches('#').trim().to_string();
            if !heading_text.is_empty() {
                headings.push((i, heading_text));
            }
        }
    }
    headings
}

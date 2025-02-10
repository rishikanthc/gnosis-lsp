// src/goto_definition.rs

use crate::db;
use log::error;
use std::path::PathBuf;
use tower_lsp::lsp_types::{Location, Position, Range};
use url::Url;

/// Asynchronously attempts to get a goto-definition Location for a wiki‑link on a given line
/// at column `col`. It parses the wiki‑link, looks up the file record in the DB (by matching the
/// virtual_path), then returns a Location that points to the start (line 0, character 0) of the
/// file (using its local path).
pub async fn get_goto_definition(line: &str, col: usize, db: &db::Database) -> Option<Location> {
    // Try to parse a wiki‑link from the line.
    if let Some((_, _, virtual_path, _alias)) = parse_wiki_link(line, col) {
        // Look up the file record using the virtual_path.
        let file_infos = match db.get_all_file_infos().await {
            Ok(infos) => infos,
            Err(e) => {
                error!("Error retrieving file infos from DB: {}", e);
                return None;
            }
        };
        if let Some(file) = file_infos
            .into_iter()
            .find(|f| f.virtual_path == virtual_path)
        {
            // Use the local file path (file.path) to generate a URI.
            let path_buf = PathBuf::from(&file.path);
            let uri = match Url::from_file_path(path_buf) {
                Ok(u) => u,
                Err(_) => {
                    error!("Could not convert local path {} to URI", file.path);
                    return None;
                }
            };
            // Create a Location that points to the start of the file.
            let loc = Location {
                uri,
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
            };
            return Some(loc);
        }
    }
    None
}

/// Parses a wiki‑link from a given line at column `col`.
/// The expected format is: `[[/virtual/path|alias]]`.
/// Returns a tuple: (start_index, end_index, virtual_path, alias)
fn parse_wiki_link(line: &str, col: usize) -> Option<(usize, usize, String, Option<String>)> {
    // Look backwards from col for the opening "[[".
    let start = line[..col].rfind("[[")?;
    // Look forward from col for the closing "]]".
    let end_rel = line[col..].find("]]")?;
    let end = col + end_rel;
    // Check that the hover position is within the wiki‑link boundaries.
    if col < start || col > end + 2 {
        return None;
    }
    // Extract the content between the brackets.
    let content = &line[start + 2..end];
    let parts: Vec<&str> = content.split('|').collect();
    if parts.is_empty() {
        return None;
    }
    let virtual_path = parts[0].trim().to_string();
    let alias = if parts.len() > 1 {
        Some(parts[1].trim().to_string())
    } else {
        None
    };
    Some((start, end + 2, virtual_path, alias))
}

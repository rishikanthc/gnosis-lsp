// src/hover_preview.rs

use crate::db;
use log::error;
use textwrap::{fill, Options};
use tokio::fs;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

/// Given a line of text and a column position, this asynchronous function checks for a valid wiki‑link.
/// If one is found, it uses the provided database to search for a file whose virtual path matches.
/// If the file is found, it reads the file using its local path and returns a Hover preview.
pub async fn get_hover_preview(line: &str, col: usize, db: &db::Database) -> Option<Hover> {
    // Attempt to parse a wiki‑link at the given column.
    if let Some((_, _, virtual_path, _alias)) = parse_wiki_link(line, col) {
        // Use the virtual path to search for the file in the database.
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
            // Use the local path (file.path) to read the file content.
            match fs::read_to_string(&file.path).await {
                Ok(content) => {
                    // Limit preview length: for example, take the first 20 lines.
                    let lines: Vec<&str> = content.lines().take(20).collect();
                    let preview_text = lines.join("\n");

                    // Wrap the text to a fixed width (e.g., 80 characters) using textwrap.
                    let options = Options::new(80);
                    let wrapped_preview = fill(&preview_text, options);

                    let hover_contents = HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("```markdown\n{}\n```", wrapped_preview),
                    });
                    return Some(Hover {
                        contents: hover_contents,
                        range: None,
                    });
                }
                Err(_) => {
                    return Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: "Unable to read file content.".to_string(),
                        }),
                        range: None,
                    });
                }
            }
        } else {
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "Wiki-link target not found in database.".to_string(),
                }),
                range: None,
            });
        }
    }
    None
}

/// Parses a wiki‑link from a given line at a specific column.
/// Expected format: `[[virtual_path|alias]]`
/// Returns (start_index, end_index, virtual_path, alias)
pub fn parse_wiki_link(line: &str, col: usize) -> Option<(usize, usize, String, Option<String>)> {
    let start = line[..col].rfind("[[")?;
    let end = col + line[col..].find("]]")?;
    if col < start || col > end + 2 {
        return None;
    }
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

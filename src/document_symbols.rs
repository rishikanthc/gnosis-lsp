// src/document_symbols.rs

use tower_lsp::lsp_types::{DocumentSymbol, Position, Range, SymbolKind};

/// Extracts document symbols (headings) from a markdown document.
/// This simple implementation scans each line for a leading '#' character.
pub fn extract_symbols(text: &str) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    for (line_index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            // Count the number of '#' to infer the heading level.
            let _level = trimmed.chars().take_while(|&c| c == '#').count();
            // Extract the heading text after the '#' characters.
            let heading_text = trimmed.trim_start_matches('#').trim();

            // Create a range covering the whole line.
            let start = Position {
                line: line_index as u32,
                character: 0,
            };
            let end = Position {
                line: line_index as u32,
                character: line.len() as u32,
            };
            let range = Range { start, end };

            // Create the document symbol.
            let symbol = DocumentSymbol {
                name: heading_text.to_string(),
                detail: None,
                // Use a suitable SymbolKind. Here we use SymbolKind::String as an example.
                kind: SymbolKind::STRING,
                range,
                selection_range: range,
                children: None,
                // Add tags as None (or a vector of tags if desired)
                tags: None,
                deprecated: None,
            };
            symbols.push(symbol);
        }
    }
    symbols
}

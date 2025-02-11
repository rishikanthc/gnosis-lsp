// src/server.rs

use crate::goto_definition;
use crate::hover_preview;
use crate::link_references;
use crate::link_references::HybridIndex;
use async_trait::async_trait;
use log::info;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::db;
use crate::document_symbols;
use crate::workspace_symbols; // <-- Import the workspace symbols module

pub struct Backend {
    pub client: Client,
    pub db: Arc<db::Database>,
    // A simple document store to cache text for open documents.
    pub documents: Mutex<HashMap<Url, String>>,
    pub ref_index: Arc<link_references::HybridIndex>,
}

impl Backend {
    pub fn new(
        client: Client,
        db: Arc<db::Database>,
        ref_index: Arc<link_references::HybridIndex>,
    ) -> Self {
        Self {
            client,
            db,
            documents: Mutex::new(HashMap::new()),
            ref_index,
        }
    }
}

#[async_trait]
impl LanguageServer for Backend {
    async fn initialize(
        &self,
        _params: InitializeParams,
    ) -> Result<InitializeResult, tower_lsp::jsonrpc::Error> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec!["[".into()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                // Advertise our document symbol provider.
                document_symbol_provider: Some(OneOf::Left(true)),
                // Advertise workspace symbol support.
                workspace_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
                    InlayHintOptions {
                        work_done_progress_options: WorkDoneProgressOptions {
                            work_done_progress: Some(false),
                        },
                        resolve_provider: Some(false),
                    },
                ))),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "Markdown Wiki-Link LSP".to_string(),
                version: Some("0.1.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Markdown Wiki-Link LSP initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<(), tower_lsp::jsonrpc::Error> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text;
        {
            let mut docs = self.documents.lock().await;
            docs.insert(uri.clone(), text);
        }
        self.client
            .log_message(MessageType::INFO, format!("Opened file: {}", uri))
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.into_iter().last() {
            let text = change.text;
            let mut docs = self.documents.lock().await;
            docs.insert(uri.clone(), text);
        }
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>, tower_lsp::jsonrpc::Error> {
        info!("Completion requested: {:?}", params);

        let infos = match self.db.get_all_file_infos().await {
            Ok(infos) => infos,
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Error querying DB for completions: {}", e),
                    )
                    .await;
                return Ok(None);
            }
        };

        let items: Vec<CompletionItem> = infos
            .into_iter()
            .map(|info| {
                let insert_text = format!("{}|{}", info.virtual_path, info.title);
                CompletionItem {
                    label: format!("{} ({})", info.title, info.virtual_path),
                    kind: Some(CompletionItemKind::FILE),
                    detail: Some(format!("Insert wiki-link for file: {}", info.virtual_path)),
                    insert_text: Some(insert_text),
                    ..Default::default()
                }
            })
            .collect();

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>, tower_lsp::jsonrpc::Error> {
        let uri = params.text_document.uri;
        let docs = self.documents.lock().await;
        let text = match docs.get(&uri) {
            Some(text) => text.clone(),
            None => {
                self.client
                    .log_message(MessageType::ERROR, format!("Document not found: {}", uri))
                    .await;
                return Ok(None);
            }
        };

        let symbols = document_symbols::extract_symbols(&text);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>, tower_lsp::jsonrpc::Error> {
        let query = params.query;
        let symbols = workspace_symbols::get_workspace_symbols(self.db.clone(), &query).await;
        Ok(Some(symbols))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>, tower_lsp::jsonrpc::Error> {
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let docs = self.documents.lock().await;
        let text = match docs.get(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        // Get the text line at the hover position.
        let lines: Vec<&str> = text.lines().collect();
        if (position.line as usize) >= lines.len() {
            return Ok(None);
        }
        let line = lines[position.line as usize];

        // Use the dedicated module to get a hover preview.
        if let Some(hover) =
            hover_preview::get_hover_preview(line, position.character as usize, self.db.as_ref())
                .await
        {
            return Ok(Some(hover));
        }
        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>, tower_lsp::jsonrpc::Error> {
        // Get the document URI and position.
        let pos = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let docs = self.documents.lock().await;
        let text = match docs.get(&uri) {
            Some(text) => text,
            None => return Ok(None),
        };
        // Get the text line at the given position.
        let lines: Vec<&str> = text.lines().collect();
        if (pos.line as usize) >= lines.len() {
            return Ok(None);
        }
        let line = lines[pos.line as usize];
        // Use our goto-definition module to get a Location.
        if let Some(loc) =
            goto_definition::get_goto_definition(line, pos.character as usize, self.db.as_ref())
                .await
        {
            Ok(Some(GotoDefinitionResponse::Scalar(loc)))
        } else {
            Ok(None)
        }
    }

    async fn code_lens(
        &self,
        params: CodeLensParams,
    ) -> Result<Option<Vec<CodeLens>>, tower_lsp::jsonrpc::Error> {
        let uri = params.text_document.uri;
        let local_path = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()));
        if local_path.is_none() {
            return Ok(None);
        }
        let local_path = local_path.unwrap();

        let file_infos = match self.db.get_all_file_infos().await {
            Ok(infos) => infos,
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Error retrieving file infos: {}", e),
                    )
                    .await;
                return Ok(None);
            }
        };

        let maybe_info = file_infos.into_iter().find(|f| f.path == local_path);
        if maybe_info.is_none() {
            return Ok(None);
        }
        let info = maybe_info.unwrap();

        // For the workspace root, we assume a WORKSPACE_ROOT env var or default to the current directory.
        let _workspace_root = std::env::var("WORKSPACE_ROOT").unwrap_or_else(|_| ".".to_string());

        // Use the hybrid index to get the reference count.
        let count = self
            .ref_index
            .get_references_count(&info.virtual_path)
            .await
            .unwrap_or(0);

        let code_lens = CodeLens {
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
            command: Some(Command {
                title: format!("Referenced {} times", count),
                command: "dummy.showReferences".to_string(),
                arguments: None,
            }),
            data: None,
        };

        Ok(Some(vec![code_lens]))
    }

    async fn inlay_hint(
        &self,
        params: InlayHintParams,
    ) -> Result<Option<Vec<InlayHint>>, tower_lsp::jsonrpc::Error> {
        // For simplicity, we ignore the requested range and always return a hint at the top.
        // In a more elaborate solution you might filter based on params.range.
        let uri = params.text_document.uri;
        // Convert the URI to a local file path.
        let local_path = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()));
        if local_path.is_none() {
            return Ok(None);
        }
        let local_path = local_path.unwrap();

        // Query your database to find the file record by matching the local path.
        let file_infos = match self.db.get_all_file_infos().await {
            Ok(infos) => infos,
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Error retrieving file infos: {}", e),
                    )
                    .await;
                return Ok(None);
            }
        };

        let maybe_info = file_infos.into_iter().find(|f| f.path == local_path);
        if maybe_info.is_none() {
            return Ok(None);
        }
        let info = maybe_info.unwrap();

        // Get the reference count using your hybrid index.
        let count = self
            .ref_index
            .get_references_count(&info.virtual_path)
            .await
            .unwrap_or(0);

        // Create an inlay hint. We place it at the very top (line 0, character 0).
        let hint = InlayHint {
            position: Position {
                line: 0,
                character: 0,
            },
            // Use the simple string variant for the label.
            label: InlayHintLabel::String(format!("Referenced {} times", count)),
            // Optionally, choose a kind. Here we use 'Other' since there’s no dedicated type.
            kind: None,
            tooltip: None,
            text_edits: None,
            data: None,
            padding_left: None,
            padding_right: None,
        };

        Ok(Some(vec![hint]))
    }
}

pub async fn run() {
    let db_instance = db::Database::new().await;
    let db_arc = Arc::new(db_instance);

    // Determine the workspace root.
    let workspace_root = std::env::var("WORKSPACE_ROOT").unwrap_or_else(|_| ".".to_string());
    // Create the hybrid index with a freshness threshold (e.g., 10 minutes).
    let ref_index = HybridIndex::new(workspace_root, Duration::from_secs(600));
    let ref_index = Arc::new(ref_index);

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) =
        LspService::build(|client| Backend::new(client, db_arc.clone(), ref_index.clone()))
            .finish();
    Server::new(stdin, stdout, socket).serve(service).await;
}

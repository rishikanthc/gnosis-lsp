// src/server.rs

use crate::hover_preview;
use async_trait::async_trait;
use log::info;
use std::collections::HashMap;
use std::sync::Arc;
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
}

impl Backend {
    pub fn new(client: Client, db: Arc<db::Database>) -> Self {
        Self {
            client,
            db,
            documents: Mutex::new(HashMap::new()),
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
}

pub async fn run() {
    let db_instance = db::Database::new().await;
    let db_arc = Arc::new(db_instance);

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) =
        LspService::build(|client| Backend::new(client, db_arc.clone())).finish();
    Server::new(stdin, stdout, socket).serve(service).await;
}

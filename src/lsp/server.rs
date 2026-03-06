use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use super::completion;
use super::diagnostics;
use super::document::DocumentStore;
use super::navigation;

pub struct JuglansLsp {
    client: Client,
    documents: Arc<RwLock<DocumentStore>>,
}

impl JuglansLsp {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(DocumentStore::new())),
        }
    }

    async fn publish_diagnostics(&self, uri: Url, content: &str, version: Option<i32>) {
        let diags = diagnostics::compute_diagnostics(content);
        self.client.publish_diagnostics(uri, diags, version).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for JuglansLsp {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["$".into(), "[".into(), ".".into(), ":".into()]),
                    ..Default::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Juglans LSP initialized")
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let text = params.text_document.text.clone();
        let version = params.text_document.version;

        self.documents
            .write()
            .await
            .open(uri.clone(), text.clone(), version);

        self.publish_diagnostics(uri, &text, Some(version)).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let version = params.text_document.version;

        if let Some(change) = params.content_changes.into_iter().last() {
            let text = change.text.clone();
            self.documents
                .write()
                .await
                .change(&uri, text.clone(), version);

            self.publish_diagnostics(uri, &text, Some(version)).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .await
            .close(&params.text_document.uri);
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(uri) {
            let items = completion::completions(doc, pos);
            if items.is_empty() {
                return Ok(None);
            }
            return Ok(Some(CompletionResponse::Array(items)));
        }
        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(uri) {
            return Ok(navigation::goto_definition(doc, uri, pos));
        }
        Ok(None)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(uri) {
            return Ok(navigation::hover(doc, pos));
        }
        Ok(None)
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

pub async fn run_server() -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(JuglansLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}

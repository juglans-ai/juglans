use std::collections::HashMap;
use tower_lsp::lsp_types::Url;

use crate::core::graph::WorkflowGraph;
use crate::core::parser::GraphParser;

/// In-memory document state for an open file
pub struct DocumentState {
    pub content: String,
    pub version: i32,
    /// Last successfully parsed graph (None if parse failed)
    pub graph: Option<WorkflowGraph>,
    /// Parse error message if parse failed
    pub parse_error: Option<String>,
}

impl DocumentState {
    pub fn new(content: String, version: i32) -> Self {
        let mut state = Self {
            content,
            version,
            graph: None,
            parse_error: None,
        };
        state.reparse();
        state
    }

    pub fn update(&mut self, content: String, version: i32) {
        self.content = content;
        self.version = version;
        self.reparse();
    }

    fn reparse(&mut self) {
        match GraphParser::parse(&self.content) {
            Ok(graph) => {
                self.graph = Some(graph);
                self.parse_error = None;
            }
            Err(e) => {
                self.parse_error = Some(e.to_string());
                // Keep old graph for completion/navigation even when parse fails
            }
        }
    }
}

/// Document store: tracks all open files
pub struct DocumentStore {
    docs: HashMap<Url, DocumentState>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            docs: HashMap::new(),
        }
    }

    pub fn open(&mut self, uri: Url, content: String, version: i32) {
        self.docs.insert(uri, DocumentState::new(content, version));
    }

    pub fn change(&mut self, uri: &Url, content: String, version: i32) {
        if let Some(doc) = self.docs.get_mut(uri) {
            doc.update(content, version);
        }
    }

    pub fn close(&mut self, uri: &Url) {
        self.docs.remove(uri);
    }

    pub fn get(&self, uri: &Url) -> Option<&DocumentState> {
        self.docs.get(uri)
    }
}

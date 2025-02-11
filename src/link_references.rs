// src/link_references.rs

use regex::escape;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time::Instant;

/// Our hybrid index maps a virtual path (String) to a tuple: (count, last_updated).
pub type ReferencesMap = HashMap<String, (usize, Instant)>;

#[derive(Clone)]
pub struct HybridIndex {
    /// In-memory index storing counts and when they were last updated.
    pub inner: Arc<RwLock<ReferencesMap>>,
    /// How long an index entry is considered fresh.
    pub freshness: Duration,
    /// The root directory of your workspace to search in (e.g. your project root).
    pub workspace_root: String,
}

impl HybridIndex {
    /// Create a new HybridIndex with a given workspace root and freshness threshold.
    pub fn new(workspace_root: String, freshness: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            freshness,
            workspace_root,
        }
    }

    /// Query the reference count for a given virtual path.
    /// If the cached value is fresh, it is returned immediately.
    /// Otherwise, ripgrep is spawned to search the workspace, and the index is updated.
    pub async fn get_references_count(
        &self,
        virtual_path: &str,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let now = Instant::now();

        // Check the in-memory index first.
        {
            let index = self.inner.read().await;
            if let Some(&(count, timestamp)) = index.get(virtual_path) {
                if now.duration_since(timestamp) < self.freshness {
                    return Ok(count);
                }
            }
        }

        // Fallback: use ripgrep to search the workspace.
        let count = self.search_with_ripgrep(virtual_path).await?;
        // Update the index.
        {
            let mut index = self.inner.write().await;
            index.insert(virtual_path.to_string(), (count, now));
        }
        Ok(count)
    }

    async fn search_with_ripgrep(
        &self,
        virtual_path: &str,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        // Escape the virtual path to match it literally.
        let escaped = escape(virtual_path);
        // Build a regex pattern that matches wiki-links starting with the virtual path.
        // Matches either: [[<virtual_path>]] or [[<virtual_path>|alias]]
        let pattern = format!(r"\[\[\s*{}(\||\]\])", escaped);

        let output = Command::new("rg")
            .arg("-o") // Only output matching parts.
            .arg("--no-heading")
            .arg("--line-number")
            .arg(&pattern)
            .arg(&self.workspace_root)
            .stdout(Stdio::piped())
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Each matching line is one reference.
        let count = stdout.lines().count();
        Ok(count)
    }
}

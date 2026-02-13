//! Backend SQLite database handlers for persisting documents.

use std::{fs::File, path::PathBuf, sync::Arc};

use anyhow::{bail, Result};
use dashmap::DashMap;
use futures::io::BufWriter;
use serde::{Deserialize, Serialize};

/// Represents a document persisted in database storage.
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct PersistedDocument {
    /// Text content of the document.
    pub text: String,
    /// Language of the document for editor syntax highlighting.
    pub language: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersistedUser {
    pub name: String,
    pub recent_documents: Vec<RecentDocument>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecentDocument {
    pub id: String,
    pub last_accessed: std::time::SystemTime,
    pub pinned: bool,
}

/// A driver for database operations wrapping a pool connection.
#[derive(Clone, Debug)]
pub struct Database {
    storage: PathBuf,
    users: Arc<DashMap<String, PersistedUser>>,
}

impl Database {
    /// Construct a new database from Postgres connection URI.
    pub async fn new(storage: PathBuf) -> Result<Self> {
        if !storage.exists() {
            std::fs::create_dir_all(&storage)?;
        }
        if !storage.join("docs").exists() {
            std::fs::create_dir_all(storage.join("docs"))?;
        }
        let mut this = Self {
            storage,
            users: Arc::new(DashMap::new()),
        };
        if this.userdata_path().exists() {
            let data = std::fs::read_to_string(this.userdata_path())?;
            this.users = Arc::new(serde_json::from_str(&data)?);
        }
        Ok(this)
    }

    /// Load the text of a document from the database.
    pub async fn load(&self, document_id: &str) -> Result<PersistedDocument> {
        let path = self.document_path(document_id);
        if !path.exists() {
            bail!("document with id {document_id} does not exist");
        }
        let text = tokio::fs::read_to_string(path).await?;
        // first line encodes the language
        let mut lines = text.lines();
        let language = lines.next().unwrap_or("markdown").to_string();
        let text = lines.collect::<Vec<_>>().join("\n");
        Ok(PersistedDocument { text, language })
    }

    /// Store the text of a document in the database.
    pub async fn store(&self, document_id: &str, document: &PersistedDocument) -> Result<()> {
        let path = self.document_path(document_id);
        let text = format!("{}\n{}", document.language, document.text);
        tokio::fs::write(path, text).await?;
        Ok(())
    }

    /// Count the number of documents in the database.
    pub async fn count(&self) -> Result<usize> {
        let mut entries = tokio::fs::read_dir(self.storage.join("docs")).await?;
        let mut count = 0;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file() {
                count += 1;
            }
        }
        Ok(count)
    }

    fn document_path(&self, document_id: &str) -> PathBuf {
        self.storage.join("docs").join(document_id)
    }

    fn userdata_path(&self) -> PathBuf {
        self.storage.join("users").with_extension("json")
    }
}

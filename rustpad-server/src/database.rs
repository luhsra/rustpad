//! Backend SQLite database handlers for persisting documents.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use dashmap::DashMap;
use rand::random;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::warn;

use crate::Identifier;
use crate::rustpad::{DocumentMeta, Visibility};

/// Represents a document persisted in database storage.
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct PersistedDocument {
    /// Metadata of the document.
    pub meta: DocumentMeta,
    /// Text content of the document.
    pub text: String,
}
impl Default for PersistedDocument {
    fn default() -> Self {
        Self {
            meta: DocumentMeta {
                language: "markdown".to_string(),
                visibility: Visibility::Public,
            },
            text: String::new(),
        }
    }
}
impl PersistedDocument {
    /// Create a new persisted document with the given text and language.
    pub fn new(text: String, language: String, visibility: Visibility) -> Self {
        Self {
            meta: DocumentMeta {
                language,
                visibility,
            },
            text,
        }
    }
}

/// Represents a user persisted in database storage.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersistedUser {
    /// Hue of the user's editor cursor.
    pub hue: u16,
    /// List of pinned documents by the user.
    pub pinned_documents: Vec<RecentDocument>,
    /// List of recently accessed documents by the user.
    pub recent_documents: Vec<RecentDocument>,
}

/// Represents a recently accessed document by a user.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecentDocument {
    /// Unique identifier of the document.
    pub id: Identifier,
    /// Timestamp of the last access to the document.
    pub last_accessed: std::time::SystemTime,
}

/// A driver for database operations wrapping a pool connection.
#[derive(Debug)]
pub struct Database {
    storage: PathBuf,
    users: DashMap<Identifier, PersistedUser>,
}

impl Database {
    /// Construct a new database from Postgres connection URI.
    pub async fn new(storage: PathBuf) -> Result<Self> {
        if !storage.exists() {
            fs::create_dir_all(&storage).await?;
        }
        let this = Self {
            storage,
            users: DashMap::new(),
        };
        fs::create_dir_all(this.document_path()).await?;
        fs::create_dir_all(this.user_path()).await?;

        let mut entries = fs::read_dir(this.user_path()).await?;
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_file()
                || entry.path().extension().and_then(|s| s.to_str()) != Some("json")
            {
                continue;
            }

            if let Some(username) = entry.path().file_stem()
                && let Some(username) = username.to_str()
                && let Ok(username) = username.parse::<Identifier>()
            {
                let user = fs::read_to_string(entry.path()).await?;
                let user: PersistedUser = serde_json::from_str(&user)?;
                this.users.insert(username, user);
            } else {
                warn!(
                    "skipping non-user file in user directory: {}",
                    entry.path().display()
                );
            }
        }
        Ok(this)
    }

    /// Construct a new database in a temporary directory for testing.
    pub async fn temporary() -> Result<Self> {
        let storage = std::env::temp_dir().join(format!("rustpad_{:x}", random::<u64>()));
        Self::new(storage).await
    }

    /// Load the text of a document from the database.
    pub async fn load_document(&self, document_id: &Identifier) -> Result<PersistedDocument> {
        let meta_path = self.document_meta_path_for(document_id);
        if meta_path.exists() {
            let meta_data = fs::read_to_string(meta_path).await?;

            let text = fs::read_to_string(self.document_path_for(document_id)).await?;
            let meta: DocumentMeta = serde_json::from_str(&meta_data)?;
            Ok(PersistedDocument { text, meta })
        } else {
            bail!("Document not found");
        }
    }

    /// Store the text of a document in the database.
    pub async fn store_document(
        &self,
        document_id: &Identifier,
        document: &PersistedDocument,
    ) -> Result<()> {
        let path = self.document_path_for(document_id);
        let meta_path = self.document_meta_path_for(document_id);
        let document = document.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            std::fs::write(path, &document.text).context("Failed to write document")?;
            std::fs::write(meta_path, serde_json::to_string_pretty(&document.meta)?)
                .context("Failed to write meta")?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    /// Count the number of documents in the database.
    pub async fn document_count(&self) -> Result<usize> {
        let mut entries = fs::read_dir(self.storage.join("docs")).await?;
        let mut count = 0;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file()
                && let Ok(_) = entry.file_name().to_string_lossy().parse::<Identifier>()
            {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Load a user's data from the database.
    pub async fn load_user(&self, username: &Identifier) -> Result<PersistedUser> {
        if let Some(user) = self.users.get(username) {
            Ok(user.clone())
        } else {
            bail!("User not found");
        }
    }

    /// Store a user's data in the database.
    pub async fn store_user(&self, username: &Identifier, user: &PersistedUser) -> Result<()> {
        self.users.insert(username.clone(), user.clone());
        let path = self.user_path_for(username);
        fs::write(path, serde_json::to_string_pretty(user)?).await?;
        Ok(())
    }

    fn document_meta_path_for(&self, document_id: &Identifier) -> PathBuf {
        self.document_path_for(document_id).with_extension("json")
    }
    fn document_path_for(&self, document_id: &Identifier) -> PathBuf {
        self.document_path().join(document_id.as_ref())
    }
    fn document_path(&self) -> PathBuf {
        self.storage.join("docs")
    }

    fn user_path(&self) -> PathBuf {
        self.storage.join("users")
    }
    fn user_path_for(&self, username: &Identifier) -> PathBuf {
        self.user_path()
            .join(username.as_ref())
            .with_extension("json")
    }
}

#[cfg(test)]
impl Drop for Database {
    fn drop(&mut self) {
        // Clean up temporary storage directories on drop.
        if self.storage.parent() == Some(std::env::temp_dir().as_path()) {
            let _ = std::fs::remove_dir_all(&self.storage);
        }
    }
}

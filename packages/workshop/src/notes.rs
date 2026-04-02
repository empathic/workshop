use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub session_id: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Optional position in conversation (entry UUID)
    pub entry_id: Option<String>,
}

/// Simple file-based notes storage
pub struct NotesStorage {
    notes_dir: PathBuf,
    cache: Arc<RwLock<HashMap<String, Vec<Note>>>>,
}

impl NotesStorage {
    pub fn new(data_dir: &Path) -> Result<Self> {
        let notes_dir = data_dir.join("notes");
        fs::create_dir_all(&notes_dir)?;

        let mut cache = HashMap::new();

        // Load existing notes
        if let Ok(entries) = fs::read_dir(&notes_dir) {
            for entry in entries.flatten() {
                if let Some(session_id) = entry.file_name().to_str()
                    && session_id.ends_with(".json")
                {
                    let session_id = session_id.trim_end_matches(".json");
                    if let Ok(content) = fs::read_to_string(entry.path())
                        && let Ok(notes) = serde_json::from_str::<Vec<Note>>(&content)
                    {
                        cache.insert(session_id.to_string(), notes);
                    }
                }
            }
        }

        info!("Loaded notes for {} conversations", cache.len());

        Ok(Self {
            notes_dir,
            cache: Arc::new(RwLock::new(cache)),
        })
    }

    pub async fn add_note(
        &self,
        session_id: &str,
        content: String,
        entry_id: Option<String>,
    ) -> Result<Note> {
        let note = Note {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            content,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
            entry_id,
        };

        let mut cache = self.cache.write().await;
        let notes = cache.entry(session_id.to_string()).or_insert_with(Vec::new);
        notes.push(note.clone());

        // Save to disk
        self.save_notes(session_id, notes).await?;

        debug!("Added note {} for session {}", note.id, session_id);
        Ok(note)
    }

    pub async fn update_note(
        &self,
        session_id: &str,
        note_id: &str,
        content: String,
    ) -> Result<()> {
        let mut cache = self.cache.write().await;
        if let Some(notes) = cache.get_mut(session_id)
            && let Some(note) = notes.iter_mut().find(|n| n.id == note_id)
        {
            note.content = content;
            note.updated_at = chrono::Utc::now().timestamp();

            // Save to disk
            self.save_notes(session_id, notes).await?;
            debug!("Updated note {} for session {}", note_id, session_id);
        }
        Ok(())
    }

    pub async fn delete_note(&self, session_id: &str, note_id: &str) -> Result<()> {
        let mut cache = self.cache.write().await;
        if let Some(notes) = cache.get_mut(session_id) {
            notes.retain(|n| n.id != note_id);

            // Save to disk
            self.save_notes(session_id, notes).await?;
            debug!("Deleted note {} from session {}", note_id, session_id);
        }
        Ok(())
    }

    pub async fn get_notes(&self, session_id: &str) -> Vec<Note> {
        let cache = self.cache.read().await;
        cache.get(session_id).cloned().unwrap_or_default()
    }

    async fn save_notes(&self, session_id: &str, notes: &[Note]) -> Result<()> {
        let file_path = self.notes_dir.join(format!("{}.json", session_id));
        let content = serde_json::to_string_pretty(notes)?;
        tokio::fs::write(file_path, content).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_storage(tmp: &std::path::Path) -> NotesStorage {
        NotesStorage::new(tmp).unwrap()
    }

    #[tokio::test]
    async fn test_add_and_get_note() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());

        let note = storage
            .add_note("session1", "Hello".into(), None)
            .await
            .unwrap();
        assert_eq!(note.session_id, "session1");
        assert_eq!(note.content, "Hello");
        assert!(!note.id.is_empty());

        let notes = storage.get_notes("session1").await;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].content, "Hello");
    }

    #[tokio::test]
    async fn test_get_notes_empty_session() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());
        let notes = storage.get_notes("nonexistent").await;
        assert!(notes.is_empty());
    }

    #[tokio::test]
    async fn test_add_multiple_notes() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());

        storage.add_note("s1", "First".into(), None).await.unwrap();
        storage.add_note("s1", "Second".into(), None).await.unwrap();
        storage
            .add_note("s2", "Other session".into(), None)
            .await
            .unwrap();

        let s1_notes = storage.get_notes("s1").await;
        assert_eq!(s1_notes.len(), 2);

        let s2_notes = storage.get_notes("s2").await;
        assert_eq!(s2_notes.len(), 1);
    }

    #[tokio::test]
    async fn test_update_note() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());

        let note = storage
            .add_note("s1", "Original".into(), None)
            .await
            .unwrap();
        storage
            .update_note("s1", &note.id, "Updated".into())
            .await
            .unwrap();

        let notes = storage.get_notes("s1").await;
        assert_eq!(notes[0].content, "Updated");
        assert!(notes[0].updated_at >= notes[0].created_at);
    }

    #[tokio::test]
    async fn test_delete_note() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());

        let note1 = storage.add_note("s1", "Keep".into(), None).await.unwrap();
        let note2 = storage
            .add_note("s1", "Delete me".into(), None)
            .await
            .unwrap();

        storage.delete_note("s1", &note2.id).await.unwrap();

        let notes = storage.get_notes("s1").await;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, note1.id);
    }

    #[tokio::test]
    async fn test_note_with_entry_id() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());

        let note = storage
            .add_note("s1", "Annotated".into(), Some("entry-uuid".into()))
            .await
            .unwrap();
        assert_eq!(note.entry_id.as_deref(), Some("entry-uuid"));
    }

    #[tokio::test]
    async fn test_persistence_across_instances() {
        let tmp = tempfile::tempdir().unwrap();

        // Write with one instance
        {
            let storage = make_storage(tmp.path());
            storage
                .add_note("s1", "Persisted".into(), None)
                .await
                .unwrap();
        }

        // Read with a new instance
        {
            let storage = make_storage(tmp.path());
            let notes = storage.get_notes("s1").await;
            assert_eq!(notes.len(), 1);
            assert_eq!(notes[0].content, "Persisted");
        }
    }

    #[tokio::test]
    async fn test_delete_from_nonexistent_session() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());
        // Should not error
        storage.delete_note("nope", "fake-id").await.unwrap();
    }

    #[tokio::test]
    async fn test_update_nonexistent_note() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());
        // Should not error
        storage
            .update_note("nope", "fake-id", "content".into())
            .await
            .unwrap();
    }
}

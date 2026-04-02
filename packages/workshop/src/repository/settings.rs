use std::collections::HashMap;

use anyhow::Result;
use sqlx::Row;

use super::ConversationRepository;

impl ConversationRepository {
    // === Server settings ===

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM server_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get("value")))
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO server_settings (key, value, updated_at) VALUES (?, ?, unixepoch())",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // === User settings ===

    pub async fn get_user_settings(&self, user_id: &str) -> Result<HashMap<String, String>> {
        let rows = sqlx::query("SELECT key, value FROM user_settings WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?;
        let mut settings = HashMap::new();
        for row in rows {
            settings.insert(row.get("key"), row.get("value"));
        }
        Ok(settings)
    }

    pub async fn set_user_setting(&self, user_id: &str, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO user_settings (user_id, key, value, updated_at) VALUES (?, ?, ?, unixepoch())",
        )
        .bind(user_id)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_user_settings(
        &self,
        user_id: &str,
        settings: &HashMap<String, String>,
    ) -> Result<()> {
        for (key, value) in settings {
            self.set_user_setting(user_id, key, value).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::User;
    use crate::repository::test_helpers;
    use chrono::Utc;

    fn make_user(id: &str, username: &str) -> User {
        let now = Utc::now().timestamp();
        User {
            id: id.to_string(),
            username: username.to_string(),
            display_name: username.to_string(),
            password_hash: "hashed".to_string(),
            is_admin: false,
            is_disabled: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn get_nonexistent_setting() {
        let repo = test_helpers::test_repository().await;
        let result = repo.get_setting("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn set_and_get_setting() {
        let repo = test_helpers::test_repository().await;
        repo.set_setting("theme", "dark").await.unwrap();

        let value = repo.get_setting("theme").await.unwrap().unwrap();
        assert_eq!(value, "dark");
    }

    #[tokio::test]
    async fn update_setting() {
        let repo = test_helpers::test_repository().await;
        repo.set_setting("theme", "dark").await.unwrap();
        repo.set_setting("theme", "light").await.unwrap();

        let value = repo.get_setting("theme").await.unwrap().unwrap();
        assert_eq!(value, "light");
    }

    #[tokio::test]
    async fn get_user_settings_empty() {
        let repo = test_helpers::test_repository().await;
        let settings = repo.get_user_settings("user-1").await.unwrap();
        assert!(settings.is_empty());
    }

    #[tokio::test]
    async fn set_and_get_user_setting() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("user-1", "testuser"))
            .await
            .unwrap();
        repo.set_user_setting("user-1", "theme", "analog")
            .await
            .unwrap();

        let settings = repo.get_user_settings("user-1").await.unwrap();
        assert_eq!(settings.get("theme").unwrap(), "analog");
    }

    #[tokio::test]
    async fn set_user_settings_bulk() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("user-1", "testuser"))
            .await
            .unwrap();

        let mut settings = std::collections::HashMap::new();
        settings.insert("theme".to_string(), "phosphor".to_string());
        settings.insert("diffEngine".to_string(), "patience".to_string());
        repo.set_user_settings("user-1", &settings).await.unwrap();

        let result = repo.get_user_settings("user-1").await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("theme").unwrap(), "phosphor");
        assert_eq!(result.get("diffEngine").unwrap(), "patience");
    }

    #[tokio::test]
    async fn upsert_user_setting() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("user-1", "testuser"))
            .await
            .unwrap();
        repo.set_user_setting("user-1", "theme", "phosphor")
            .await
            .unwrap();
        repo.set_user_setting("user-1", "theme", "analog")
            .await
            .unwrap();

        let settings = repo.get_user_settings("user-1").await.unwrap();
        assert_eq!(settings.get("theme").unwrap(), "analog");
    }
}

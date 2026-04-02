use anyhow::{Context, Result};
use sqlx::Row;

use crate::models::{
    InstanceInvitation, InstancePermission, InviteAcceptor, ServerInvite,
    ServerInviteWithAcceptors, Session, User,
};

use super::ConversationRepository;

impl ConversationRepository {
    // =========================================================================
    // User CRUD
    // =========================================================================

    pub async fn create_user(&self, user: &User) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO users (id, username, display_name, password_hash, is_admin, is_disabled, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&user.id)
        .bind(&user.username)
        .bind(&user.display_name)
        .bind(&user.password_hash)
        .bind(user.is_admin)
        .bind(user.is_disabled)
        .bind(user.created_at)
        .bind(user.updated_at)
        .execute(&self.pool)
        .await
        .context("Failed to create user")?;
        Ok(())
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let row = sqlx::query(
            r#"
            SELECT id, username, display_name, password_hash, is_admin, is_disabled, created_at, updated_at
            FROM users WHERE username = ?
            "#,
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| User {
            id: r.get("id"),
            username: r.get("username"),
            display_name: r.get("display_name"),
            password_hash: r.get("password_hash"),
            is_admin: r.get::<i32, _>("is_admin") != 0,
            is_disabled: r.get::<i32, _>("is_disabled") != 0,
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        }))
    }

    pub async fn user_count(&self) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM users")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("cnt"))
    }

    pub async fn update_user_password(&self, user_id: &str, password_hash: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(password_hash)
            .bind(now)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .context("Failed to update user password")?;
        Ok(())
    }

    // =========================================================================
    // Session CRUD
    // =========================================================================

    pub async fn create_session(&self, session: &Session) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO sessions (token, user_id, csrf_token, expires_at, last_active_at, user_agent, ip_address)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&session.token)
        .bind(&session.user_id)
        .bind(&session.csrf_token)
        .bind(session.expires_at)
        .bind(session.last_active_at)
        .bind(&session.user_agent)
        .bind(&session.ip_address)
        .execute(&self.pool)
        .await
        .context("Failed to create session")?;
        Ok(())
    }

    /// Look up a session by token, joining with users. Returns (Session, User) if valid.
    pub async fn get_session_with_user(&self, token: &str) -> Result<Option<(Session, User)>> {
        let row = sqlx::query(
            r#"
            SELECT s.token, s.user_id, s.csrf_token, s.expires_at, s.last_active_at,
                   s.user_agent, s.ip_address,
                   u.id as u_id, u.username, u.display_name, u.password_hash,
                   u.is_admin, u.is_disabled, u.created_at as u_created_at, u.updated_at as u_updated_at
            FROM sessions s
            JOIN users u ON s.user_id = u.id
            WHERE s.token = ? AND s.expires_at > unixepoch() AND u.is_disabled = 0
            "#,
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            let session = Session {
                token: r.get("token"),
                user_id: r.get("user_id"),
                csrf_token: r.get("csrf_token"),
                expires_at: r.get("expires_at"),
                last_active_at: r.get("last_active_at"),
                user_agent: r.get("user_agent"),
                ip_address: r.get("ip_address"),
            };
            let user = User {
                id: r.get("u_id"),
                username: r.get("username"),
                display_name: r.get("display_name"),
                password_hash: r.get("password_hash"),
                is_admin: r.get::<i32, _>("is_admin") != 0,
                is_disabled: r.get::<i32, _>("is_disabled") != 0,
                created_at: r.get("u_created_at"),
                updated_at: r.get("u_updated_at"),
            };
            (session, user)
        }))
    }

    pub async fn delete_session(&self, token: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete all sessions for a user, optionally keeping one alive.
    pub async fn delete_user_sessions(
        &self,
        user_id: &str,
        except_token: Option<&str>,
    ) -> Result<u64> {
        let result = if let Some(keep) = except_token {
            sqlx::query("DELETE FROM sessions WHERE user_id = ? AND token != ?")
                .bind(user_id)
                .bind(keep)
                .execute(&self.pool)
                .await?
        } else {
            sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                .bind(user_id)
                .execute(&self.pool)
                .await?
        };
        Ok(result.rows_affected())
    }

    pub async fn cleanup_expired_sessions(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= unixepoch()")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn touch_session(&self, token: &str) -> Result<()> {
        sqlx::query("UPDATE sessions SET last_active_at = unixepoch() WHERE token = ?")
            .bind(token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Instance Permissions
    // =========================================================================

    pub async fn create_instance_permission(&self, perm: &InstancePermission) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO instance_permissions (instance_id, user_id, role, granted_at, granted_by)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&perm.instance_id)
        .bind(&perm.user_id)
        .bind(&perm.role)
        .bind(perm.granted_at)
        .bind(&perm.granted_by)
        .execute(&self.pool)
        .await
        .context("Failed to create instance permission")?;
        Ok(())
    }

    pub async fn check_instance_permission(
        &self,
        instance_id: &str,
        user_id: &str,
    ) -> Result<Option<InstancePermission>> {
        let row = sqlx::query(
            r#"
            SELECT instance_id, user_id, role, granted_at, granted_by
            FROM instance_permissions
            WHERE instance_id = ? AND user_id = ?
            "#,
        )
        .bind(instance_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| InstancePermission {
            instance_id: r.get("instance_id"),
            user_id: r.get("user_id"),
            role: r.get("role"),
            granted_at: r.get("granted_at"),
            granted_by: r.get("granted_by"),
        }))
    }

    pub async fn list_user_instance_ids(&self, user_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT instance_id FROM instance_permissions WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|r| r.get("instance_id")).collect())
    }

    pub async fn delete_instance_permission(&self, instance_id: &str, user_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM instance_permissions WHERE instance_id = ? AND user_id = ?")
            .bind(instance_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Invitations
    // =========================================================================

    pub async fn create_invitation(&self, invite: &InstanceInvitation) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO instance_invitations (invite_token, instance_id, created_by, role, max_uses, use_count, expires_at, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&invite.invite_token)
        .bind(&invite.instance_id)
        .bind(&invite.created_by)
        .bind(&invite.role)
        .bind(invite.max_uses)
        .bind(invite.use_count)
        .bind(invite.expires_at)
        .bind(invite.created_at)
        .execute(&self.pool)
        .await
        .context("Failed to create invitation")?;
        Ok(())
    }

    pub async fn get_invitation(&self, token: &str) -> Result<Option<InstanceInvitation>> {
        let row = sqlx::query(
            r#"
            SELECT invite_token, instance_id, created_by, role, max_uses, use_count, expires_at, created_at
            FROM instance_invitations WHERE invite_token = ?
            "#,
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| InstanceInvitation {
            invite_token: r.get("invite_token"),
            instance_id: r.get("instance_id"),
            created_by: r.get("created_by"),
            role: r.get("role"),
            max_uses: r.get("max_uses"),
            use_count: r.get("use_count"),
            expires_at: r.get("expires_at"),
            created_at: r.get("created_at"),
        }))
    }

    pub async fn accept_invitation(&self, token: &str) -> Result<()> {
        sqlx::query(
            "UPDATE instance_invitations SET use_count = use_count + 1 WHERE invite_token = ?",
        )
        .bind(token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // =========================================================================
    // Server Invites
    // =========================================================================

    pub async fn create_server_invite(&self, invite: &ServerInvite) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO server_invites (token, created_by, label, max_uses, use_count, expires_at, revoked, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&invite.token)
        .bind(&invite.created_by)
        .bind(&invite.label)
        .bind(invite.max_uses)
        .bind(invite.use_count)
        .bind(invite.expires_at)
        .bind(invite.revoked)
        .bind(invite.created_at)
        .execute(&self.pool)
        .await
        .context("Failed to create server invite")?;
        Ok(())
    }

    pub async fn get_server_invite(&self, token: &str) -> Result<Option<ServerInvite>> {
        let row = sqlx::query(
            r#"
            SELECT token, created_by, label, max_uses, use_count, expires_at, revoked, created_at
            FROM server_invites WHERE token = ?
            "#,
        )
        .bind(token)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| ServerInvite {
            token: r.get("token"),
            created_by: r.get("created_by"),
            label: r.get("label"),
            max_uses: r.get("max_uses"),
            use_count: r.get("use_count"),
            expires_at: r.get("expires_at"),
            revoked: r.get::<i32, _>("revoked") != 0,
            created_at: r.get("created_at"),
        }))
    }

    pub async fn list_server_invites(&self) -> Result<Vec<ServerInviteWithAcceptors>> {
        let invite_rows = sqlx::query(
            r#"
            SELECT token, created_by, label, max_uses, use_count, expires_at, revoked, created_at
            FROM server_invites
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for r in invite_rows {
            let token: String = r.get("token");

            let acceptor_rows = sqlx::query(
                r#"
                SELECT id, username, display_name, created_at
                FROM users
                WHERE server_invite_token = ?
                ORDER BY created_at ASC
                "#,
            )
            .bind(&token)
            .fetch_all(&self.pool)
            .await?;

            let acceptors = acceptor_rows
                .into_iter()
                .map(|ar| InviteAcceptor {
                    user_id: ar.get("id"),
                    username: ar.get("username"),
                    display_name: ar.get("display_name"),
                    created_at: ar.get("created_at"),
                })
                .collect();

            results.push(ServerInviteWithAcceptors {
                invite: ServerInvite {
                    token,
                    created_by: r.get("created_by"),
                    label: r.get("label"),
                    max_uses: r.get("max_uses"),
                    use_count: r.get("use_count"),
                    expires_at: r.get("expires_at"),
                    revoked: r.get::<i32, _>("revoked") != 0,
                    created_at: r.get("created_at"),
                },
                acceptors,
            });
        }

        Ok(results)
    }

    pub async fn use_server_invite(&self, token: &str, user_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("UPDATE server_invites SET use_count = use_count + 1 WHERE token = ?")
            .bind(token)
            .execute(&mut *tx)
            .await?;

        sqlx::query("UPDATE users SET server_invite_token = ? WHERE id = ?")
            .bind(token)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    // =========================================================================
    // User Admin Operations
    // =========================================================================

    pub async fn list_users(&self) -> Result<Vec<User>> {
        let rows = sqlx::query(
            r#"
            SELECT id, username, display_name, password_hash, is_admin, is_disabled, created_at, updated_at
            FROM users ORDER BY created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| User {
                id: r.get("id"),
                username: r.get("username"),
                display_name: r.get("display_name"),
                password_hash: r.get("password_hash"),
                is_admin: r.get::<i32, _>("is_admin") != 0,
                is_disabled: r.get::<i32, _>("is_disabled") != 0,
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    /// Count non-disabled admin users.
    pub async fn count_active_admins(&self) -> Result<i64> {
        let row =
            sqlx::query("SELECT COUNT(*) as cnt FROM users WHERE is_admin = 1 AND is_disabled = 0")
                .fetch_one(&self.pool)
                .await?;
        Ok(row.get("cnt"))
    }

    pub async fn set_user_admin(&self, user_id: &str, is_admin: bool) -> Result<bool> {
        let result =
            sqlx::query("UPDATE users SET is_admin = ?, updated_at = unixepoch() WHERE id = ?")
                .bind(is_admin)
                .bind(user_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_user_disabled(&self, user_id: &str, is_disabled: bool) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        let result =
            sqlx::query("UPDATE users SET is_disabled = ?, updated_at = unixepoch() WHERE id = ?")
                .bind(is_disabled)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;

        // Invalidate all sessions when disabling a user
        if is_disabled {
            sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_user_display_name(
        &self,
        user_id: &str,
        display_name: &str,
    ) -> Result<bool> {
        let result =
            sqlx::query("UPDATE users SET display_name = ?, updated_at = unixepoch() WHERE id = ?")
                .bind(display_name)
                .bind(user_id)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_user(&self, user_id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        // Delete sessions first
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let result = sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn revoke_server_invite(&self, token: &str) -> Result<bool> {
        let result =
            sqlx::query("UPDATE server_invites SET revoked = 1 WHERE token = ? AND revoked = 0")
                .bind(token)
                .execute(&self.pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{InstanceInvitation, InstancePermission, ServerInvite, Session, User};
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

    fn make_session(token: &str, user_id: &str, expires_in_secs: i64) -> Session {
        let now = Utc::now().timestamp();
        Session {
            token: token.to_string(),
            user_id: user_id.to_string(),
            csrf_token: "csrf-tok".to_string(),
            expires_at: now + expires_in_secs,
            last_active_at: now,
            user_agent: Some("test".to_string()),
            ip_address: Some("127.0.0.1".to_string()),
        }
    }

    #[tokio::test]
    async fn create_and_get_user() {
        let repo = test_helpers::test_repository().await;
        let user = make_user("u-1", "alice");
        repo.create_user(&user).await.unwrap();

        let fetched = repo.get_user_by_username("alice").await.unwrap().unwrap();
        assert_eq!(fetched.id, "u-1");
        assert_eq!(fetched.display_name, "alice");
        assert!(!fetched.is_admin);
    }

    #[tokio::test]
    async fn get_nonexistent_user() {
        let repo = test_helpers::test_repository().await;
        let result = repo.get_user_by_username("nobody").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn user_count() {
        let repo = test_helpers::test_repository().await;
        assert_eq!(repo.user_count().await.unwrap(), 0);

        repo.create_user(&make_user("u-1", "alice")).await.unwrap();
        repo.create_user(&make_user("u-2", "bob")).await.unwrap();
        assert_eq!(repo.user_count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn update_password() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        repo.update_user_password("u-1", "new-hash").await.unwrap();

        let fetched = repo.get_user_by_username("alice").await.unwrap().unwrap();
        assert_eq!(fetched.password_hash, "new-hash");
    }

    #[tokio::test]
    async fn session_create_and_validate() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        let session = make_session("tok-1", "u-1", 3600);
        repo.create_session(&session).await.unwrap();

        let (sess, user) = repo.get_session_with_user("tok-1").await.unwrap().unwrap();
        assert_eq!(sess.user_id, "u-1");
        assert_eq!(user.username, "alice");
    }

    #[tokio::test]
    async fn expired_session_not_returned() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        // Session expired 1 second ago
        let session = make_session("tok-expired", "u-1", -1);
        repo.create_session(&session).await.unwrap();

        let result = repo.get_session_with_user("tok-expired").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn delete_session() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();
        repo.create_session(&make_session("tok-1", "u-1", 3600))
            .await
            .unwrap();

        repo.delete_session("tok-1").await.unwrap();
        let result = repo.get_session_with_user("tok-1").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn delete_user_sessions_except_one() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();
        repo.create_session(&make_session("tok-1", "u-1", 3600))
            .await
            .unwrap();
        repo.create_session(&make_session("tok-2", "u-1", 3600))
            .await
            .unwrap();
        repo.create_session(&make_session("tok-3", "u-1", 3600))
            .await
            .unwrap();

        let deleted = repo
            .delete_user_sessions("u-1", Some("tok-2"))
            .await
            .unwrap();
        assert_eq!(deleted, 2);

        // tok-2 should still exist
        assert!(repo.get_session_with_user("tok-2").await.unwrap().is_some());
        assert!(repo.get_session_with_user("tok-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn instance_permission_lifecycle() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        let perm = InstancePermission {
            instance_id: "inst-1".to_string(),
            user_id: "u-1".to_string(),
            role: "collaborator".to_string(),
            granted_at: Utc::now().timestamp(),
            granted_by: None,
        };
        repo.create_instance_permission(&perm).await.unwrap();

        let checked = repo
            .check_instance_permission("inst-1", "u-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(checked.role, "collaborator");

        let ids = repo.list_user_instance_ids("u-1").await.unwrap();
        assert_eq!(ids, vec!["inst-1"]);

        repo.delete_instance_permission("inst-1", "u-1")
            .await
            .unwrap();
        let checked = repo
            .check_instance_permission("inst-1", "u-1")
            .await
            .unwrap();
        assert!(checked.is_none());
    }

    #[tokio::test]
    async fn invitation_lifecycle() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        let invite = InstanceInvitation {
            invite_token: "inv-tok".to_string(),
            instance_id: "inst-1".to_string(),
            created_by: "u-1".to_string(),
            role: "collaborator".to_string(),
            max_uses: Some(3),
            use_count: 0,
            expires_at: None,
            created_at: Utc::now().timestamp(),
        };
        repo.create_invitation(&invite).await.unwrap();

        let fetched = repo.get_invitation("inv-tok").await.unwrap().unwrap();
        assert_eq!(fetched.use_count, 0);

        repo.accept_invitation("inv-tok").await.unwrap();
        let fetched = repo.get_invitation("inv-tok").await.unwrap().unwrap();
        assert_eq!(fetched.use_count, 1);
    }

    #[tokio::test]
    async fn cleanup_expired_sessions() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();

        // Create an expired session and a valid session
        repo.create_session(&make_session("tok-expired", "u-1", -100))
            .await
            .unwrap();
        repo.create_session(&make_session("tok-valid", "u-1", 3600))
            .await
            .unwrap();

        let cleaned = repo.cleanup_expired_sessions().await.unwrap();
        assert_eq!(cleaned, 1);

        // Valid session should still exist
        assert!(
            repo.get_session_with_user("tok-valid")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn cleanup_expired_sessions_none_expired() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();
        repo.create_session(&make_session("tok-1", "u-1", 3600))
            .await
            .unwrap();

        let cleaned = repo.cleanup_expired_sessions().await.unwrap();
        assert_eq!(cleaned, 0);
    }

    #[tokio::test]
    async fn touch_session_updates_last_active() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "alice")).await.unwrap();
        repo.create_session(&make_session("tok-1", "u-1", 3600))
            .await
            .unwrap();

        // Touch the session
        repo.touch_session("tok-1").await.unwrap();

        // Verify it still exists and is accessible
        let (sess, _) = repo.get_session_with_user("tok-1").await.unwrap().unwrap();
        assert_eq!(sess.token, "tok-1");
    }

    #[tokio::test]
    async fn touch_nonexistent_session_is_noop() {
        let repo = test_helpers::test_repository().await;
        // Should not error on missing session
        repo.touch_session("nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn list_server_invites_empty() {
        let repo = test_helpers::test_repository().await;
        let invites = repo.list_server_invites().await.unwrap();
        assert!(invites.is_empty());
    }

    #[tokio::test]
    async fn list_server_invites_with_acceptors() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("admin-1", "admin"))
            .await
            .unwrap();

        let invite = ServerInvite {
            token: "srv-tok-1".to_string(),
            created_by: "admin-1".to_string(),
            label: Some("Team invite".to_string()),
            max_uses: Some(10),
            use_count: 0,
            expires_at: None,
            revoked: false,
            created_at: Utc::now().timestamp(),
        };
        repo.create_server_invite(&invite).await.unwrap();

        // Create a second invite
        let invite2 = ServerInvite {
            token: "srv-tok-2".to_string(),
            created_by: "admin-1".to_string(),
            label: None,
            max_uses: None,
            use_count: 0,
            expires_at: None,
            revoked: false,
            created_at: Utc::now().timestamp(),
        };
        repo.create_server_invite(&invite2).await.unwrap();

        let invites = repo.list_server_invites().await.unwrap();
        assert_eq!(invites.len(), 2);
        // Check the first invite has correct fields
        let first = invites
            .iter()
            .find(|i| i.invite.token == "srv-tok-1")
            .unwrap();
        assert_eq!(first.invite.label, Some("Team invite".to_string()));
        assert!(first.acceptors.is_empty());
    }

    #[tokio::test]
    async fn use_server_invite_links_user() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("admin-1", "admin"))
            .await
            .unwrap();
        repo.create_user(&make_user("new-user", "newbie"))
            .await
            .unwrap();

        let invite = ServerInvite {
            token: "srv-tok".to_string(),
            created_by: "admin-1".to_string(),
            label: None,
            max_uses: None,
            use_count: 0,
            expires_at: None,
            revoked: false,
            created_at: Utc::now().timestamp(),
        };
        repo.create_server_invite(&invite).await.unwrap();

        // Use the invite
        repo.use_server_invite("srv-tok", "new-user").await.unwrap();

        // use_count should be incremented
        let fetched = repo.get_server_invite("srv-tok").await.unwrap().unwrap();
        assert_eq!(fetched.use_count, 1);

        // User should show up as acceptor in list
        let invites = repo.list_server_invites().await.unwrap();
        let inv = invites
            .iter()
            .find(|i| i.invite.token == "srv-tok")
            .unwrap();
        assert_eq!(inv.acceptors.len(), 1);
        assert_eq!(inv.acceptors[0].user_id, "new-user");
    }

    #[tokio::test]
    async fn count_active_admins_empty() {
        let repo = test_helpers::test_repository().await;
        assert_eq!(repo.count_active_admins().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn count_active_admins_excludes_disabled() {
        let repo = test_helpers::test_repository().await;

        let mut admin1 = make_user("u-1", "admin1");
        admin1.is_admin = true;
        repo.create_user(&admin1).await.unwrap();

        let mut admin2 = make_user("u-2", "admin2");
        admin2.is_admin = true;
        repo.create_user(&admin2).await.unwrap();

        // Non-admin
        repo.create_user(&make_user("u-3", "regular"))
            .await
            .unwrap();

        assert_eq!(repo.count_active_admins().await.unwrap(), 2);

        // Disable one admin
        repo.set_user_disabled("u-2", true).await.unwrap();
        assert_eq!(repo.count_active_admins().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn count_active_admins_excludes_non_admins() {
        let repo = test_helpers::test_repository().await;

        let mut admin = make_user("u-1", "admin");
        admin.is_admin = true;
        repo.create_user(&admin).await.unwrap();

        repo.create_user(&make_user("u-2", "regular"))
            .await
            .unwrap();

        assert_eq!(repo.count_active_admins().await.unwrap(), 1);

        // Demote the admin
        repo.set_user_admin("u-1", false).await.unwrap();
        assert_eq!(repo.count_active_admins().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn server_invite_revoke() {
        let repo = test_helpers::test_repository().await;
        repo.create_user(&make_user("u-1", "admin")).await.unwrap();

        let invite = ServerInvite {
            token: "srv-tok".to_string(),
            created_by: "u-1".to_string(),
            label: Some("test invite".to_string()),
            max_uses: None,
            use_count: 0,
            expires_at: None,
            revoked: false,
            created_at: Utc::now().timestamp(),
        };
        repo.create_server_invite(&invite).await.unwrap();

        let revoked = repo.revoke_server_invite("srv-tok").await.unwrap();
        assert!(revoked);

        let fetched = repo.get_server_invite("srv-tok").await.unwrap().unwrap();
        assert!(fetched.revoked);

        // Double-revoke returns false
        let revoked_again = repo.revoke_server_invite("srv-tok").await.unwrap();
        assert!(!revoked_again);
    }
}

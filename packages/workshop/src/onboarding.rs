//! First-run onboarding: creates the initial admin account before the HTTP server starts.
//!
//! Supports two modes:
//! - **Interactive**: prompts the operator in the terminal
//! - **Headless**: reads from `WORKSHOP_ADMIN_USERNAME` / `WORKSHOP_ADMIN_PASSWORD` env vars

use anyhow::{Result, bail};
use tracing::info;

use crate::auth::hash_password;
use crate::config::AuthConfig;
use crate::models::User;
use crate::repository::ConversationRepository;

/// Run first-time admin setup if auth is enabled and no users exist.
///
/// This blocks before the HTTP listener starts, ensuring the server
/// operator creates the admin account from their terminal (or env vars).
pub async fn maybe_run_onboarding(
    repository: &ConversationRepository,
    auth_config: &AuthConfig,
) -> Result<()> {
    if !auth_config.enabled {
        return Ok(());
    }

    let user_count = repository.user_count().await?;
    if user_count > 0 {
        return Ok(());
    }

    info!("No users found -- running first-time admin setup");

    // Check for headless env vars first
    let env_username = std::env::var("WORKSHOP_ADMIN_USERNAME").ok();
    let env_password = std::env::var("WORKSHOP_ADMIN_PASSWORD").ok();

    let (username, password, display_name) = match (env_username, env_password) {
        (Some(u), Some(p)) => {
            let dn = std::env::var("WORKSHOP_ADMIN_DISPLAY_NAME")
                .ok()
                .unwrap_or_else(|| u.clone());
            info!("Creating admin from environment variables (headless mode)");
            (u, p, dn)
        }
        _ => {
            // Interactive terminal prompt
            interactive_prompt().await?
        }
    };

    create_headless_admin(repository, username, password, display_name).await
}

/// Validate credentials and create the initial admin account.
///
/// Extracted so tests can call it directly without env var races.
async fn create_headless_admin(
    repository: &ConversationRepository,
    username: String,
    password: String,
    display_name: String,
) -> Result<()> {
    let username = username.trim().to_string();
    if username.len() < 2 || username.len() > 64 {
        bail!("Username must be 2-64 characters");
    }
    if password.len() < 8 {
        bail!("Password must be at least 8 characters");
    }

    let password_hash = hash_password(&password)?;
    let display_name = if display_name.trim().is_empty() {
        username.clone()
    } else {
        display_name.trim().to_string()
    };

    let user = User {
        id: uuid::Uuid::new_v4().to_string(),
        username: username.clone(),
        display_name,
        password_hash,
        is_admin: true,
        is_disabled: false,
        created_at: chrono::Utc::now().timestamp(),
        updated_at: chrono::Utc::now().timestamp(),
    };

    repository.create_user(&user).await?;
    repository
        .set_setting("allow_registration", "false")
        .await?;

    info!("Admin account '{}' created successfully", username);
    info!("Registration has been locked down -- new users must be invited");

    Ok(())
}

/// Reset an admin account's password via interactive terminal prompt.
pub async fn reset_admin(repository: &ConversationRepository) -> Result<()> {
    let (username, password) = tokio::task::spawn_blocking(|| -> Result<(String, String)> {
        use std::io::{self, BufRead, Write};

        let stdin = io::stdin();
        let stdout = io::stdout();

        println!();
        println!("=== Workshop: Reset Admin Password ===");
        println!();

        // Username
        print!("  Admin username: ");
        stdout.lock().flush()?;
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let username = line.trim().to_string();

        // New password
        let password = loop {
            let pw = rpassword::prompt_password("  New password (min 8 chars): ")
                .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;
            if pw.len() < 8 {
                println!("  (too short -- minimum 8 characters)");
                continue;
            }

            let confirm = rpassword::prompt_password("  Confirm new password: ")
                .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;
            if confirm != pw {
                println!("  (passwords don't match, try again)");
                continue;
            }

            break pw;
        };

        println!();

        Ok((username, password))
    })
    .await??;

    let user = repository
        .get_user_by_username(&username)
        .await?
        .ok_or_else(|| anyhow::anyhow!("No user found with username '{}'", username))?;

    if !user.is_admin {
        bail!("User '{}' is not an admin", username);
    }

    let password_hash = hash_password(&password)?;
    repository
        .update_user_password(&user.id, &password_hash)
        .await?;

    // Invalidate all existing sessions for this user
    let invalidated = repository.delete_user_sessions(&user.id, None).await?;
    if invalidated > 0 {
        info!("Invalidated {} existing session(s)", invalidated);
    }

    info!("Password reset for admin '{}'", username);

    Ok(())
}

/// Prompt the operator interactively in the terminal.
async fn interactive_prompt() -> Result<(String, String, String)> {
    tokio::task::spawn_blocking(|| {
        use std::io::{self, BufRead, Write};

        let stdin = io::stdin();
        let stdout = io::stdout();

        println!();
        println!("=== Workshop: First-Time Setup ===");
        println!();
        println!("Auth is enabled but no accounts exist yet.");
        println!("Create the initial admin account:");
        println!();

        // Username
        let username = loop {
            print!("  Username: ");
            stdout.lock().flush()?;
            let mut line = String::new();
            stdin.lock().read_line(&mut line)?;
            let trimmed = line.trim().to_string();
            if trimmed.len() >= 2 && trimmed.len() <= 64 {
                break trimmed;
            }
            println!("  (must be 2-64 characters)");
        };

        // Display name
        print!("  Display name [{}]: ", &username);
        stdout.lock().flush()?;
        let mut dn_line = String::new();
        stdin.lock().read_line(&mut dn_line)?;
        let display_name = {
            let t = dn_line.trim();
            if t.is_empty() {
                username.clone()
            } else {
                t.to_string()
            }
        };

        // Password (hidden input)
        let password = loop {
            let pw = rpassword::prompt_password("  Password (min 8 chars): ")
                .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;
            if pw.len() < 8 {
                println!("  (too short -- minimum 8 characters)");
                continue;
            }

            let confirm = rpassword::prompt_password("  Confirm password: ")
                .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;
            if confirm != pw {
                println!("  (passwords don't match, try again)");
                continue;
            }

            break pw;
        };

        println!();

        Ok((username, password, display_name))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthConfig;
    use crate::repository::test_helpers;

    fn auth_config(enabled: bool) -> AuthConfig {
        AuthConfig {
            enabled,
            session_ttl_secs: 3600,
            allow_registration: true,
            https: false,
        }
    }

    #[tokio::test]
    async fn auth_disabled_skips_onboarding() {
        let repo = test_helpers::test_repository().await;
        maybe_run_onboarding(&repo, &auth_config(false))
            .await
            .unwrap();
        // No users should be created
        assert_eq!(repo.user_count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn existing_users_skips_onboarding() {
        let repo = test_helpers::test_repository().await;
        // Create a user first
        let user = User {
            id: "u1".to_string(),
            username: "admin".to_string(),
            display_name: "Admin".to_string(),
            password_hash: hash_password("testpassword").unwrap(),
            is_admin: true,
            is_disabled: false,
            created_at: chrono::Utc::now().timestamp(),
            updated_at: chrono::Utc::now().timestamp(),
        };
        repo.create_user(&user).await.unwrap();

        maybe_run_onboarding(&repo, &auth_config(true))
            .await
            .unwrap();
        // Should still have only the original user
        assert_eq!(repo.user_count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn headless_creates_admin() {
        let repo = test_helpers::test_repository().await;

        create_headless_admin(
            &repo,
            "headless_admin".into(),
            "securepassword123".into(),
            "Headless Admin".into(),
        )
        .await
        .unwrap();

        // Admin should be created
        assert_eq!(repo.user_count().await.unwrap(), 1);
        let user = repo
            .get_user_by_username("headless_admin")
            .await
            .unwrap()
            .unwrap();
        assert!(user.is_admin);
        assert_eq!(user.display_name, "Headless Admin");

        // Registration should be locked
        let allow_reg = repo.get_setting("allow_registration").await.unwrap();
        assert_eq!(allow_reg.as_deref(), Some("false"));
    }

    #[tokio::test]
    async fn headless_validation_short_username() {
        let repo = test_helpers::test_repository().await;

        let result =
            create_headless_admin(&repo, "a".into(), "securepassword123".into(), String::new())
                .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("2-64"));
    }

    #[tokio::test]
    async fn headless_validation_short_password() {
        let repo = test_helpers::test_repository().await;

        let result =
            create_headless_admin(&repo, "admin".into(), "short".into(), String::new()).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("8 characters"));
    }

    #[tokio::test]
    async fn headless_display_name_defaults_to_username() {
        let repo = test_helpers::test_repository().await;

        create_headless_admin(
            &repo,
            "myuser".into(),
            "securepassword123".into(),
            String::new(),
        )
        .await
        .unwrap();

        let user = repo.get_user_by_username("myuser").await.unwrap().unwrap();
        assert_eq!(user.display_name, "myuser");
    }
}

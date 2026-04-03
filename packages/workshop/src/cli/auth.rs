//! `work auth enable|disable|status` — manage authentication on a running daemon.

use anyhow::{Context, Result};

use super::daemon::{self, DaemonInfo};
use workshop::config::{FileConfig, WorkshopConfig, load_config};

/// `work auth enable` — write auth.enabled=true to config.toml, restart server,
/// and if no users exist prompt for admin credentials.
pub async fn enable_command(config: &WorkshopConfig) -> Result<()> {
    set_auth_enabled(config, true)?;
    eprintln!("Auth enabled in {}", config.config_toml_path().display());

    // If daemon is running, restart it so the new config takes effect
    if let Some(daemon) = daemon::check_daemon(config) {
        if daemon::health_check_pub(&daemon).await {
            trigger_restart(&daemon).await?;
            eprintln!("Server restarting with auth enabled.");

            // Wait briefly for the server to come back up
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            // Check if users exist; if not, prompt for admin account
            maybe_create_admin(&daemon).await?;
        }
    } else {
        eprintln!("Daemon not running. Auth will be enabled on next start.");
    }

    Ok(())
}

/// `work auth disable` — write auth.enabled=false to config.toml, restart server.
pub async fn disable_command(config: &WorkshopConfig) -> Result<()> {
    set_auth_enabled(config, false)?;
    eprintln!("Auth disabled in {}", config.config_toml_path().display());

    if let Some(daemon) = daemon::check_daemon(config) {
        if daemon::health_check_pub(&daemon).await {
            trigger_restart(&daemon).await?;
            eprintln!("Server restarting with auth disabled.");
        }
    } else {
        eprintln!("Daemon not running. Auth will be disabled on next start.");
    }

    Ok(())
}

/// `work auth status` — read config and print current auth state.
/// If the daemon is running, fetches effective config from `/api/admin/config`.
pub async fn status_command(config: &WorkshopConfig) -> Result<()> {
    // Try to get live config from running daemon
    if let Some(daemon) = daemon::check_daemon(config)
        && daemon::health_check_pub(&daemon).await
    {
        let url = format!("{}/api/admin/config", daemon.base_url());
        if let Ok(resp) = reqwest::get(&url).await
            && let Ok(cfg) = resp.json::<serde_json::Value>().await
        {
            let auth = cfg
                .get("auth_enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let https = cfg.get("https").and_then(|v| v.as_bool()).unwrap_or(false);
            let host = cfg
                .get("host")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let port = cfg.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
            let profile = cfg
                .get("profile")
                .and_then(|v| v.as_str())
                .unwrap_or("(none)");

            eprintln!("Daemon running (pid {} on {}:{})", daemon.pid, host, port);
            eprintln!("  profile:  {}", profile);
            if auth {
                eprintln!("  auth:     enabled");
                eprintln!("  https:    {}", https);
            } else {
                eprintln!("  auth:     disabled");
            }
            return Ok(());
        }
    }

    // Fallback: read from config file
    let fc: FileConfig = load_config(&config.data_dir, None)
        .extract()
        .unwrap_or_default();

    if fc.auth.enabled {
        eprintln!("Auth: enabled");
        eprintln!("  session_ttl_secs:    {}", fc.auth.session_ttl_secs);
        eprintln!("  allow_registration:  {}", fc.auth.allow_registration);
        eprintln!("  https:               {}", fc.auth.https);
    } else {
        eprintln!("Auth: disabled");
    }

    let toml_path = config.config_toml_path();
    if toml_path.exists() {
        eprintln!("  config file: {}", toml_path.display());
    } else {
        eprintln!("  config file: (none — using defaults)");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read-modify-write `config.toml` to set `[auth] enabled`.
fn set_auth_enabled(config: &WorkshopConfig, enabled: bool) -> Result<()> {
    let path = config.config_toml_path();

    // Read existing TOML or start fresh
    let mut doc = if path.exists() {
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        contents
            .parse::<toml::Table>()
            .with_context(|| format!("Failed to parse {}", path.display()))?
    } else {
        toml::Table::new()
    };

    let auth_table = doc
        .entry("auth")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .context("[auth] in config.toml is not a table")?;

    auth_table.insert("enabled".to_string(), toml::Value::Boolean(enabled));

    let serialized = toml::to_string_pretty(&doc).context("Failed to serialize config.toml")?;
    std::fs::write(&path, serialized)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

/// Hit the daemon's restart endpoint over loopback.
async fn trigger_restart(daemon: &DaemonInfo) -> Result<()> {
    let url = format!("{}/api/admin/restart", daemon.base_url());
    let resp = reqwest::Client::new()
        .post(&url)
        .send()
        .await
        .context("Failed to reach daemon restart endpoint")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Restart request failed: {} {}", status, body);
    }

    Ok(())
}

/// If auth is now enabled and no users exist, interactively create the admin account
/// by calling the daemon's register endpoint.
async fn maybe_create_admin(daemon: &DaemonInfo) -> Result<()> {
    // Check via /api/auth/me whether setup is needed
    let me_url = format!("{}/api/auth/me", daemon.base_url());
    let resp = reqwest::get(&me_url)
        .await
        .context("Failed to check auth status")?;

    let body: serde_json::Value = resp.json().await.context("Failed to parse /api/auth/me")?;

    if body.get("needs_setup").and_then(|v| v.as_bool()) != Some(true) {
        return Ok(());
    }

    eprintln!();
    eprintln!("No admin account exists. Create one now:");
    eprintln!();

    let (username, password) = tokio::task::spawn_blocking(|| -> Result<(String, String)> {
        use std::io::{self, BufRead, Write};

        let stdin = io::stdin();
        let stdout = io::stdout();

        let username = loop {
            print!("  Username: ");
            stdout.lock().flush()?;
            let mut line = String::new();
            stdin.lock().read_line(&mut line)?;
            let trimmed = line.trim().to_string();
            if trimmed.len() >= 2 && trimmed.len() <= 64 {
                break trimmed;
            }
            eprintln!("  (must be 2-64 characters)");
        };

        let password = loop {
            let pw = rpassword::prompt_password("  Password (min 8 chars): ")
                .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;
            if pw.len() < 8 {
                eprintln!("  (too short — minimum 8 characters)");
                continue;
            }

            let confirm = rpassword::prompt_password("  Confirm password: ")
                .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;
            if confirm != pw {
                eprintln!("  (passwords don't match, try again)");
                continue;
            }

            break pw;
        };

        Ok((username, password))
    })
    .await??;

    // Register via the daemon's API (first user becomes admin)
    let register_url = format!("{}/api/auth/register", daemon.base_url());
    let resp = reqwest::Client::new()
        .post(&register_url)
        .json(&serde_json::json!({
            "username": username,
            "password": password,
        }))
        .send()
        .await
        .context("Failed to register admin")?;

    if resp.status().is_success() {
        eprintln!();
        eprintln!("Admin account '{}' created.", username);
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Registration failed: {} {}", status, body);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config() -> (tempfile::TempDir, WorkshopConfig) {
        let dir = tempfile::tempdir().unwrap();
        let config = WorkshopConfig::new(Some(dir.path().to_path_buf())).unwrap();
        (dir, config)
    }

    #[test]
    fn set_auth_enabled_creates_config_file() {
        let (_dir, config) = temp_config();
        let path = config.config_toml_path();
        assert!(!path.exists());

        set_auth_enabled(&config, true).unwrap();
        assert!(path.exists());

        let contents = std::fs::read_to_string(&path).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        let auth = doc["auth"].as_table().unwrap();
        assert_eq!(auth["enabled"].as_bool(), Some(true));
    }

    #[test]
    fn set_auth_enabled_toggle() {
        let (_dir, config) = temp_config();

        set_auth_enabled(&config, true).unwrap();
        let contents = std::fs::read_to_string(config.config_toml_path()).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        assert_eq!(doc["auth"]["enabled"].as_bool(), Some(true));

        set_auth_enabled(&config, false).unwrap();
        let contents = std::fs::read_to_string(config.config_toml_path()).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        assert_eq!(doc["auth"]["enabled"].as_bool(), Some(false));
    }

    #[test]
    fn set_auth_enabled_preserves_existing_keys() {
        let (_dir, config) = temp_config();
        let path = config.config_toml_path();

        // Write some pre-existing config
        std::fs::write(
            &path,
            "[server]\nport = 8080\n\n[auth]\nsession_ttl_secs = 3600\n",
        )
        .unwrap();

        set_auth_enabled(&config, true).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        assert_eq!(doc["server"]["port"].as_integer(), Some(8080));
        assert_eq!(doc["auth"]["session_ttl_secs"].as_integer(), Some(3600));
        assert_eq!(doc["auth"]["enabled"].as_bool(), Some(true));
    }

    #[test]
    fn set_auth_enabled_overwrites_existing_enabled() {
        let (_dir, config) = temp_config();
        let path = config.config_toml_path();

        std::fs::write(&path, "[auth]\nenabled = false\n").unwrap();

        set_auth_enabled(&config, true).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let doc: toml::Table = contents.parse().unwrap();
        assert_eq!(doc["auth"]["enabled"].as_bool(), Some(true));
    }
}

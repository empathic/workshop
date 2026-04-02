/// Run a git command in the given working directory, returning stdout on success.
pub async fn run_git(working_dir: &str, args: &[&str]) -> Result<String, String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(working_dir)
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

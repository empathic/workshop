use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};

const PROXY_URL: &str = "https://workshop.hotline.empathic.dev";
const PROXY_TOKEN: &str = "nkCk16ewj5YDPqhZ7FSBHM44+3y5F5HpH0FdvVrIO8A=";

#[derive(Deserialize)]
pub struct BugReportRequest {
    pub title: String,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct BugReportResponse {
    pub url: String,
}

pub async fn create_bug_report(
    Json(req): Json<BugReportRequest>,
) -> Result<Json<BugReportResponse>, (StatusCode, String)> {
    let title = req.title;
    let description = req.description.unwrap_or_default();

    let t1 = title.clone();
    let d1 = description.clone();
    let linear = tokio::task::spawn_blocking(move || {
        let mut issue = hotln::linear(PROXY_URL);
        issue.with_token(PROXY_TOKEN).title(&t1);
        if !d1.is_empty() {
            issue.text(&d1);
        }
        issue.create()
    });

    let t2 = title;
    let d2 = description;
    let github = tokio::task::spawn_blocking(move || {
        let mut issue = hotln::github(PROXY_URL);
        issue.with_token(PROXY_TOKEN).title(&t2);
        if !d2.is_empty() {
            issue.text(&d2);
        }
        issue.create()
    });

    let (linear_result, github_result) = tokio::join!(linear, github);

    match &linear_result {
        Ok(Err(e)) => tracing::warn!("Linear hotline failed: {e}"),
        Err(e) => tracing::warn!("Linear hotline task panicked: {e}"),
        _ => {}
    }

    let github_url = match github_result {
        Ok(Ok(url)) => url,
        Ok(Err(e)) => {
            tracing::warn!("GitHub hotline failed: {e}");
            return Err((
                StatusCode::BAD_GATEWAY,
                "Failed to create bug report".to_string(),
            ));
        }
        Err(e) => {
            tracing::warn!("GitHub hotline task panicked: {e}");
            return Err((
                StatusCode::BAD_GATEWAY,
                "Failed to create bug report".to_string(),
            ));
        }
    };

    Ok(Json(BugReportResponse { url: github_url }))
}

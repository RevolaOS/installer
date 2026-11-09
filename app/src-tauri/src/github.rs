// SPDX-License-Identifier: GPL-3.0-only
//! Release metadata from the GitHub REST API. Release bodies are the only changelog.
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
pub struct Release {
    pub html_url: String,
    pub body: Option<String>,
    pub draft: bool,
    pub published_at: Option<String>,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("revola-installer/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

fn get(client: &reqwest::blocking::Client, url: &str) -> reqwest::blocking::RequestBuilder {
    client
        .get(url)
        .header("Accept", "application/vnd.github+json")
}

/// Every release of `owner/repo`, drafts included.
pub fn releases(repo: &str) -> Result<Vec<Release>, String> {
    let client = client()?;
    let url = format!("https://api.github.com/repos/{repo}/releases");
    let mut releases = Vec::new();
    for page in 1.. {
        let response = get(&client, &url)
            .query(&[("per_page", 100), ("page", page)])
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|e| format!("Could not check RevolaOS releases: {e}"))?;
        let batch: Vec<Release> = serde_json::from_reader(response).map_err(|e| e.to_string())?;
        let last_page = batch.len() < 100;
        releases.extend(batch);
        if last_page {
            break;
        }
    }
    Ok(releases)
}

pub fn tag_url(repo: &str, tag: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases/tags/{tag}")
}

pub fn release_by_tag(repo: &str, tag: &str) -> Result<Release, String> {
    let response = get(&client()?, &tag_url(repo, tag))
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| e.to_string())?;
    serde_json::from_reader(response).map_err(|e| e.to_string())
}

/// A release body with surrounding whitespace removed; `None` when empty.
pub fn notes(release: &Release) -> Option<String> {
    release
        .body
        .as_deref()
        .map(str::trim)
        .filter(|body| !body.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_lookup_uses_the_release_tag() {
        assert_eq!(
            tag_url("RevolaOS/installer", "v0.2.0"),
            "https://api.github.com/repos/RevolaOS/installer/releases/tags/v0.2.0"
        );
    }

    #[test]
    fn empty_bodies_are_no_notes() {
        let release = |body| Release {
            html_url: String::new(),
            body,
            draft: false,
            published_at: None,
            assets: vec![],
        };
        assert_eq!(notes(&release(None)), None);
        assert_eq!(notes(&release(Some("  \n".into()))), None);
        assert_eq!(
            notes(&release(Some("\n## Fixes\n".into()))).as_deref(),
            Some("## Fixes")
        );
    }
}

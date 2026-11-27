use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};

use crate::catalog::ContentKind;
use crate::error::{MubError, Result};

const GITHUB_API_BASE: &str = "https://api.github.com";
const USER_AGENT_VALUE: &str = concat!("ModsUndBots/", env!("CARGO_PKG_VERSION"));

pub const DEFAULT_GITHUB_OWNER: &str = "GermanCGArtist";

/// Priority repositories featured when browsing/searching GitHub mods.
pub const FEATURED_GITHUB_REPOS: &[&str] = &[
    "GermanCGArtist/manhattan",
    "GermanCGArtist/vitruvius",
    "GermanCGArtist/heavymatica",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhOwner {
    pub login: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhLicense {
    pub name: Option<String>,
    pub spdx_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhRepo {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub owner: GhOwner,
    pub description: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub stargazers_count: u64,
    #[serde(default)]
    pub topics: Vec<String>,
    pub license: Option<GhLicense>,
    #[serde(default)]
    pub private: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhRelease {
    pub id: u64,
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub html_url: String,
    pub published_at: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhAsset {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub download_count: u64,
}

#[derive(Clone)]
pub struct GitHubClient {
    http: reqwest::Client,
    token: Option<String>,
    custom_repos: Vec<String>,
}

impl GitHubClient {
    pub fn new(token: Option<String>, custom_repos: Vec<String>) -> Self {
        let token = token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build GitHub reqwest client");
        Self {
            http,
            token,
            custom_repos,
        }
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    pub fn custom_repos(&self) -> &[String] {
        &self.custom_repos
    }

    fn apply_api_headers(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut req = req
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(ACCEPT, "application/vnd.github+json");
        if let Some(tok) = &self.token {
            req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
        }
        req
    }

    pub async fn get_repo(&self, owner: &str, repo: &str) -> Result<GhRepo> {
        let url = format!("{GITHUB_API_BASE}/repos/{owner}/{repo}");
        let resp = self.apply_api_headers(self.http.get(&url)).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(MubError::Download(format!(
                "GitHub repo {owner}/{repo} failed ({status}): {body}"
            )));
        }
        Ok(resp.json::<GhRepo>().await?)
    }

    pub async fn get_readme(&self, owner: &str, repo: &str) -> Result<Option<String>> {
        let url = format!("{GITHUB_API_BASE}/repos/{owner}/{repo}/readme");
        let mut req = self
            .http
            .get(&url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(ACCEPT, "application/vnd.github.raw+json");
        if let Some(tok) = &self.token {
            req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
        }
        let resp = req.send().await?;
        if resp.status().as_u16() == 404 {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Ok(None);
        }
        Ok(Some(resp.text().await?))
    }

    pub async fn list_releases(
        &self,
        owner: &str,
        repo: &str,
        per_page: u32,
    ) -> Result<Vec<GhRelease>> {
        let url = format!(
            "{GITHUB_API_BASE}/repos/{owner}/{repo}/releases?per_page={}",
            per_page.clamp(1, 100)
        );
        let resp = self.apply_api_headers(self.http.get(&url)).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(MubError::Download(format!(
                "GitHub releases for {owner}/{repo} failed ({status}): {body}"
            )));
        }
        let releases = resp.json::<Vec<GhRelease>>().await?;
        Ok(releases.into_iter().filter(|r| !r.draft).collect())
    }

    /// Lists repositories for `owner`. When a token is configured, queries
    /// `/user/repos?affiliation=owner` first so private repos owned by the
    /// authenticated user are included, falling back to `/users/{owner}/repos`.
    pub async fn list_owner_repos(&self, owner: &str, per_page: u32) -> Result<Vec<GhRepo>> {
        let limit = per_page.clamp(1, 100);
        if self.token.is_some() {
            let auth_url = format!(
                "{GITHUB_API_BASE}/user/repos?affiliation=owner&sort=updated&per_page={limit}"
            );
            if let Ok(resp) = self.apply_api_headers(self.http.get(&auth_url)).send().await
                && resp.status().is_success()
                && let Ok(repos) = resp.json::<Vec<GhRepo>>().await
            {
                let filtered: Vec<GhRepo> = repos
                    .into_iter()
                    .filter(|r| r.owner.login.eq_ignore_ascii_case(owner))
                    .collect();
                if !filtered.is_empty() {
                    return Ok(filtered);
                }
            }
        }

        let url =
            format!("{GITHUB_API_BASE}/users/{owner}/repos?sort=updated&per_page={limit}");
        let resp = self.apply_api_headers(self.http.get(&url)).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(MubError::Download(format!(
                "GitHub user repos for {owner} failed ({status}): {body}"
            )));
        }
        Ok(resp.json::<Vec<GhRepo>>().await?)
    }

    pub async fn download_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let mut req = self.http.get(url).header(USER_AGENT, USER_AGENT_VALUE);
        if url.starts_with("https://api.github.com/") {
            req = req.header(ACCEPT, "application/octet-stream");
            if let Some(tok) = &self.token {
                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
            }
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(MubError::Download(format!(
                "GitHub asset download failed ({status}): {body}"
            )));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub fn asset_download_url(&self, asset: &GhAsset) -> String {
        if self.token.is_some() && !asset.url.is_empty() {
            asset.url.clone()
        } else {
            asset.browser_download_url.clone()
        }
    }
}

/// Parses `"owner/repo"`, `"https://github.com/owner/repo..."`, or a featured
/// short name like `"manhattan"` / `"vitruvius"` / `"heavymatica"` into `(owner, repo)`.
pub fn parse_repo_spec(raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let path = if let Some(rest) = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("github.com/"))
    {
        rest
    } else {
        trimmed
    };

    let mut parts = path.split('/').filter(|s| !s.is_empty());
    match (parts.next(), parts.next()) {
        (Some(owner), Some(repo)) => {
            let repo = repo.strip_suffix(".git").unwrap_or(repo);
            Some((owner.to_string(), repo.to_string()))
        }
        (Some(single), None) => {
            for featured in FEATURED_GITHUB_REPOS {
                if let Some((owner, repo)) = featured.split_once('/')
                    && repo.eq_ignore_ascii_case(single)
                {
                    return Some((owner.to_string(), repo.to_string()));
                }
            }
            None
        }
        _ => None,
    }
}

pub fn is_relevant_asset(filename: &str, kind: ContentKind) -> bool {
    let lower = filename.to_ascii_lowercase();
    match kind {
        ContentKind::Mod => {
            lower.ends_with(".jar")
                && !lower.ends_with("-sources.jar")
                && !lower.ends_with("-javadoc.jar")
                && !lower.ends_with("-dev.jar")
                && !lower.ends_with("-all-dev.jar")
        }
        ContentKind::Modpack => lower.ends_with(".mrpack") || lower.ends_with(".zip"),
        ContentKind::ResourcePack | ContentKind::Shader | ContentKind::DataPack => {
            lower.ends_with(".zip")
        }
    }
}

/// Best-effort extraction of Minecraft versions (e.g. `1.21.4`) and mod loaders
/// (`fabric`, `neoforge`, `forge`, `quilt`) from release tags, names, and asset filenames.
pub fn extract_mc_versions_and_loaders(text: &str) -> (Vec<String>, Vec<String>) {
    let mut mc_versions = Vec::new();
    let mut loaders = Vec::new();
    let lower = text.to_ascii_lowercase();

    for loader in ["neoforge", "fabric", "quilt", "forge"] {
        if lower.contains(loader) {
            // Avoid matching "forge" solely as a substring of "neoforge"
            if loader == "forge"
                && !lower
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(|tok| tok == "forge")
            {
                continue;
            }
            if !loaders.iter().any(|l| l == loader) {
                loaders.push(loader.to_string());
            }
        }
    }

    // Scan tokens for Minecraft version patterns like `1.21.4`, `1.20.1`, `1.21`
    for token in lower.split(|c: char| !c.is_ascii_digit() && c != '.') {
        let token = token.trim_matches('.');
        let mut segs = token.split('.');
        let Some(maj) = segs.next() else { continue };
        let Some(min_str) = segs.next() else { continue };
        if maj != "1" {
            continue;
        }
        let Ok(min_num) = min_str.parse::<u32>() else {
            continue;
        };
        // Minecraft 1.7 through 1.29 avoids false positives on mod versions like 1.0.0 / 1.2.0
        if !(7..=29).contains(&min_num) {
            continue;
        }
        let ver = match segs.next() {
            Some(patch) if !patch.is_empty() && patch.chars().all(|c| c.is_ascii_digit()) => {
                if segs.next().is_some() {
                    continue;
                }
                format!("1.{min_num}.{patch}")
            }
            Some(_) => continue,
            None => format!("1.{min_num}"),
        };
        if !mc_versions.contains(&ver) {
            mc_versions.push(ver);
        }
    }

    (mc_versions, loaders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_specs_and_featured_names() {
        assert_eq!(
            parse_repo_spec("GermanCGArtist/manhattan"),
            Some(("GermanCGArtist".into(), "manhattan".into()))
        );
        assert_eq!(
            parse_repo_spec("https://github.com/GermanCGArtist/vitruvius/releases"),
            Some(("GermanCGArtist".into(), "vitruvius".into()))
        );
        assert_eq!(
            parse_repo_spec("heavymatica"),
            Some(("GermanCGArtist".into(), "heavymatica".into()))
        );
        assert_eq!(parse_repo_spec("sodium"), None);
    }

    #[test]
    fn extracts_mc_versions_and_loaders() {
        let (vers, loaders) =
            extract_mc_versions_and_loaders("manhattan-fabric-1.21.4-v1.0.2.jar");
        assert_eq!(vers, vec!["1.21.4"]);
        assert_eq!(loaders, vec!["fabric"]);

        let (vers2, loaders2) =
            extract_mc_versions_and_loaders("vitruvius-neoforge-1.21.1-0.3.0.jar");
        assert_eq!(vers2, vec!["1.21.1"]);
        assert_eq!(loaders2, vec!["neoforge"]);
    }

    #[test]
    fn filters_relevant_mod_assets() {
        assert!(is_relevant_asset("manhattan-1.21.4.jar", ContentKind::Mod));
        assert!(!is_relevant_asset(
            "manhattan-1.21.4-sources.jar",
            ContentKind::Mod
        ));
        assert!(!is_relevant_asset(
            "manhattan-1.21.4-dev.jar",
            ContentKind::Mod
        ));
        assert!(is_relevant_asset("pack.zip", ContentKind::ResourcePack));
    }
}

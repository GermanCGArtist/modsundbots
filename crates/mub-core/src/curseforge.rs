use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::error::{MubError, Result};

const CURSEFORGE_API_BASE: &str = "https://api.curseforge.com/v1";

pub const MC_GAME_ID: u32 = 432;
pub const CLASS_MODS: u32 = 6;
pub const CLASS_RESOURCEPACKS: u32 = 12;
pub const CLASS_MODPACKS: u32 = 4471;
pub const CLASS_SHADERS: u32 = 6552;
pub const CLASS_DATAPACKS: u32 = 6945;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CfPagination {
    pub index: u32,
    #[serde(rename = "pageSize")]
    pub page_size: u32,
    #[serde(rename = "resultCount")]
    pub result_count: u32,
    #[serde(rename = "totalCount")]
    pub total_count: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CfSearchResponse {
    pub data: Vec<CfMod>,
    pub pagination: CfPagination,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfMod {
    pub id: u64,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub class_id: Option<u32>,
    #[serde(default)]
    pub download_count: u64,
    #[serde(default)]
    pub thumbs_up_count: u64,
    #[serde(default)]
    pub logo: Option<CfAsset>,
    #[serde(default)]
    pub authors: Vec<CfAuthor>,
    #[serde(default)]
    pub categories: Vec<CfCategory>,
    #[serde(default)]
    pub links: Option<CfLinks>,
    #[serde(default)]
    pub screenshots: Vec<CfAsset>,
    #[serde(default)]
    pub latest_files: Vec<CfFile>,
    #[serde(default)]
    pub latest_files_indexes: Vec<CfFileIndex>,
    #[serde(default)]
    pub date_modified: String,
    #[serde(default)]
    pub allow_mod_distribution: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfAsset {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CfAuthor {
    #[serde(default)]
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfCategory {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfLinks {
    #[serde(default)]
    pub website_url: Option<String>,
    #[serde(default)]
    pub wiki_url: Option<String>,
    #[serde(default)]
    pub issues_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: u64,
    pub mod_id: u64,
    #[serde(default)]
    pub display_name: String,
    pub file_name: String,
    #[serde(default = "default_release_type")]
    pub release_type: u32,
    #[serde(default)]
    pub file_status: u32,
    #[serde(default)]
    pub file_date: String,
    #[serde(default)]
    pub file_length: u64,
    #[serde(default)]
    pub download_count: u64,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub hashes: Vec<CfHash>,
    #[serde(default)]
    pub file_fingerprint: u64,
    #[serde(default)]
    pub dependencies: Vec<CfDependency>,
}

fn default_release_type() -> u32 {
    1
}

impl CfFile {
    pub fn release_type_str(&self) -> &'static str {
        match self.release_type {
            1 => "release",
            2 => "beta",
            3 => "alpha",
            _ => "release",
        }
    }

    pub fn sha1_hash(&self) -> Option<&str> {
        self.hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| h.value.as_str())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CfHash {
    pub value: String,
    pub algo: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfDependency {
    pub mod_id: u64,
    pub relation_type: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFileIndex {
    pub game_version: String,
    pub file_id: u64,
    pub filename: String,
    #[serde(default = "default_release_type")]
    pub release_type: u32,
    #[serde(default)]
    pub mod_loader: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFingerprintResult {
    #[serde(default)]
    pub exact_matches: Vec<CfFingerprintMatch>,
    #[serde(default)]
    pub unmatched_fingerprints: Vec<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFingerprintMatch {
    pub id: u64,
    pub file: CfFile,
    #[serde(default)]
    pub latest_files: Vec<CfFile>,
}

#[derive(Debug, Deserialize)]
struct CfDataWrapper<T> {
    data: T,
}

#[derive(Clone)]
pub struct CurseForgeClient {
    http: Client,
    api_key: String,
}

impl CurseForgeClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let http = Client::builder()
            .user_agent("ModsUndBots/0.2.0 (https://github.com/modsundbots)")
            .default_headers(headers)
            .pool_max_idle_per_host(16)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            http,
            api_key: api_key.into(),
        }
    }

    fn check_status(status: StatusCode, body: &str, context: &str) -> Result<()> {
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(MubError::CurseForgeAuth(format!(
                "{context}: HTTP {status} ({body})"
            )));
        }
        if !status.is_success() {
            return Err(MubError::Download(format!(
                "CurseForge {context} failed (HTTP {status}): {body}"
            )));
        }
        Ok(())
    }

    pub async fn test_api_key(&self) -> Result<bool> {
        let resp = self
            .http
            .get(format!("{CURSEFORGE_API_BASE}/games/{MC_GAME_ID}"))
            .header("x-api-key", &self.api_key)
            .send()
            .await?;
        let status = resp.status();
        if status.is_success() {
            Ok(true)
        } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            Ok(false)
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(MubError::Download(format!(
                "CurseForge API key test failed (HTTP {status}): {text}"
            )))
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn search_mods(
        &self,
        class_id: u32,
        query: &str,
        game_version: Option<&str>,
        mod_loader_type: Option<u32>,
        sort_field: u32,
        index: u32,
        page_size: u32,
    ) -> Result<CfSearchResponse> {
        let clamped_size = page_size.clamp(1, 50);
        let mut params: Vec<(&str, String)> = vec![
            ("gameId", MC_GAME_ID.to_string()),
            ("classId", class_id.to_string()),
            ("sortField", sort_field.to_string()),
            ("sortOrder", "desc".to_string()),
            ("index", index.to_string()),
            ("pageSize", clamped_size.to_string()),
        ];
        if !query.trim().is_empty() {
            params.push(("searchFilter", query.trim().to_string()));
        }
        if let Some(gv) = game_version
            && !gv.trim().is_empty()
        {
            params.push(("gameVersion", gv.trim().to_string()));
        }
        if let Some(loader_id) = mod_loader_type {
            params.push(("modLoaderType", loader_id.to_string()));
        }

        let resp = self
            .http
            .get(format!("{CURSEFORGE_API_BASE}/mods/search"))
            .header("x-api-key", &self.api_key)
            .query(&params)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        Self::check_status(status, &text, "search")?;
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn get_mod(&self, mod_id: u64) -> Result<CfMod> {
        let resp = self
            .http
            .get(format!("{CURSEFORGE_API_BASE}/mods/{mod_id}"))
            .header("x-api-key", &self.api_key)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        Self::check_status(status, &text, &format!("get_mod({mod_id})"))?;
        let wrapped: CfDataWrapper<CfMod> = serde_json::from_str(&text)?;
        Ok(wrapped.data)
    }

    pub async fn get_mods(&self, mod_ids: &[u64]) -> Result<Vec<CfMod>> {
        if mod_ids.is_empty() {
            return Ok(Vec::new());
        }
        let resp = self
            .http
            .post(format!("{CURSEFORGE_API_BASE}/mods"))
            .header("x-api-key", &self.api_key)
            .json(&serde_json::json!({ "modIds": mod_ids }))
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        Self::check_status(status, &text, "get_mods")?;
        let wrapped: CfDataWrapper<Vec<CfMod>> = serde_json::from_str(&text)?;
        Ok(wrapped.data)
    }

    pub async fn get_mod_description(&self, mod_id: u64) -> Result<String> {
        let resp = self
            .http
            .get(format!("{CURSEFORGE_API_BASE}/mods/{mod_id}/description"))
            .header("x-api-key", &self.api_key)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        Self::check_status(status, &text, &format!("get_mod_description({mod_id})"))?;
        let wrapped: CfDataWrapper<String> = serde_json::from_str(&text)?;
        Ok(wrapped.data)
    }

    pub async fn get_mod_files(
        &self,
        mod_id: u64,
        game_version: Option<&str>,
        mod_loader_type: Option<u32>,
        index: u32,
        page_size: u32,
    ) -> Result<Vec<CfFile>> {
        let clamped_size = page_size.clamp(1, 50);
        let mut params: Vec<(&str, String)> = vec![
            ("index", index.to_string()),
            ("pageSize", clamped_size.to_string()),
        ];
        if let Some(gv) = game_version
            && !gv.trim().is_empty()
        {
            params.push(("gameVersion", gv.trim().to_string()));
        }
        if let Some(loader_id) = mod_loader_type {
            params.push(("modLoaderType", loader_id.to_string()));
        }

        let resp = self
            .http
            .get(format!("{CURSEFORGE_API_BASE}/mods/{mod_id}/files"))
            .header("x-api-key", &self.api_key)
            .query(&params)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        Self::check_status(status, &text, &format!("get_mod_files({mod_id})"))?;
        let wrapped: CfDataWrapper<Vec<CfFile>> = serde_json::from_str(&text)?;
        Ok(wrapped.data)
    }

    pub async fn get_file_changelog(&self, mod_id: u64, file_id: u64) -> Result<String> {
        let resp = self
            .http
            .get(format!(
                "{CURSEFORGE_API_BASE}/mods/{mod_id}/files/{file_id}/changelog"
            ))
            .header("x-api-key", &self.api_key)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        Self::check_status(
            status,
            &text,
            &format!("get_file_changelog({mod_id}, {file_id})"),
        )?;
        let wrapped: CfDataWrapper<String> = serde_json::from_str(&text)?;
        Ok(wrapped.data)
    }

    pub async fn get_files_by_ids(&self, file_ids: &[u64]) -> Result<Vec<CfFile>> {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }
        let resp = self
            .http
            .post(format!("{CURSEFORGE_API_BASE}/mods/files"))
            .header("x-api-key", &self.api_key)
            .json(&serde_json::json!({ "fileIds": file_ids }))
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        Self::check_status(status, &text, "get_files_by_ids")?;
        let wrapped: CfDataWrapper<Vec<CfFile>> = serde_json::from_str(&text)?;
        Ok(wrapped.data)
    }

    /// Fetch the download URL for a file. Returns `Ok(None)` when the author has
    /// opted out of third-party API distribution.
    pub async fn get_download_url(&self, mod_id: u64, file_id: u64) -> Result<Option<String>> {
        let resp = self
            .http
            .get(format!(
                "{CURSEFORGE_API_BASE}/mods/{mod_id}/files/{file_id}/download-url"
            ))
            .header("x-api-key", &self.api_key)
            .send()
            .await?;
        let status = resp.status();
        if status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status == StatusCode::UNAUTHORIZED {
            let body = resp.text().await.unwrap_or_default();
            return Err(MubError::CurseForgeAuth(format!(
                "get_download_url({mod_id}, {file_id}): HTTP {status} ({body})"
            )));
        }
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(MubError::Download(format!(
                "CurseForge get_download_url failed (HTTP {status}): {text}"
            )));
        }
        let wrapped: CfDataWrapper<Option<String>> = serde_json::from_str(&text)?;
        Ok(wrapped.data.filter(|u| !u.trim().is_empty()))
    }

    pub async fn match_fingerprints(&self, fingerprints: &[u32]) -> Result<CfFingerprintResult> {
        if fingerprints.is_empty() {
            return Ok(CfFingerprintResult {
                exact_matches: Vec::new(),
                unmatched_fingerprints: Vec::new(),
            });
        }
        let resp = self
            .http
            .post(format!("{CURSEFORGE_API_BASE}/fingerprints/{MC_GAME_ID}"))
            .header("x-api-key", &self.api_key)
            .json(&serde_json::json!({ "fingerprints": fingerprints }))
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        Self::check_status(status, &text, "match_fingerprints")?;
        let wrapped: CfDataWrapper<CfFingerprintResult> = serde_json::from_str(&text)?;
        Ok(wrapped.data)
    }

    pub async fn download_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(MubError::Download(format!(
                "GET {url} failed with status {}",
                resp.status()
            )));
        }
        Ok(resp.bytes().await?.to_vec())
    }
}

/// Compute CurseForge's MurmurHash2 fingerprint:
/// strips ASCII whitespace bytes (9, 10, 13, 32) and runs 32-bit little-endian
/// MurmurHash2 with `seed = 1`, `m = 0x5bd1e995`, `r = 24`.
pub fn compute_murmur2_fingerprint(bytes: &[u8]) -> u32 {
    const M: u32 = 0x5bd1_e995;
    const R: u32 = 24;
    const SEED: u32 = 1;

    #[inline]
    fn is_stripped(b: u8) -> bool {
        matches!(b, 9 | 10 | 13 | 32)
    }

    let normalized_len = bytes.iter().filter(|&&b| !is_stripped(b)).count() as u32;
    let mut h: u32 = SEED ^ normalized_len;

    let mut chunk = [0u8; 4];
    let mut chunk_len = 0usize;

    for &b in bytes {
        if is_stripped(b) {
            continue;
        }
        chunk[chunk_len] = b;
        chunk_len += 1;
        if chunk_len == 4 {
            let mut k = u32::from_le_bytes(chunk);
            k = k.wrapping_mul(M);
            k ^= k >> R;
            k = k.wrapping_mul(M);
            h = h.wrapping_mul(M);
            h ^= k;
            chunk_len = 0;
        }
    }

    match chunk_len {
        3 => {
            h ^= (chunk[2] as u32) << 16;
            h ^= (chunk[1] as u32) << 8;
            h ^= chunk[0] as u32;
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= (chunk[1] as u32) << 8;
            h ^= chunk[0] as u32;
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= chunk[0] as u32;
            h = h.wrapping_mul(M);
        }
        _ => {}
    }

    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

#[cfg(test)]
mod tests {
    use super::compute_murmur2_fingerprint;

    #[test]
    fn murmur2_strips_whitespace_bytes() {
        let clean = b"hello_curseforge_mod_data";
        let with_ws = b"h e\tl\nl\ro_c u r s e f o r g e _ m o d _ d a t a \r\n\t ";
        assert_eq!(
            compute_murmur2_fingerprint(clean),
            compute_murmur2_fingerprint(with_ws)
        );
    }

    #[test]
    fn murmur2_known_vectors() {
        // Empty / whitespace-only: h = 1 ^ 0 = 1; final mix on 1:
        // 1 ^= 0 -> 1; 1 * 0x5bd1e995 = 0x5bd1e995; 0x5bd1e995 ^ (0x5bd1e995 >> 15) = 0x5bd1e995 ^ 0xb7a3 = 0x5bd15e36
        assert_eq!(compute_murmur2_fingerprint(b""), 0x5bd1_5e36);
        assert_eq!(compute_murmur2_fingerprint(b" \t\r\n"), 0x5bd1_5e36);
        // Single byte and 4-byte aligned inputs produce stable distinct hashes
        assert_ne!(
            compute_murmur2_fingerprint(b"abcd"),
            compute_murmur2_fingerprint(b"abce")
        );
        assert_ne!(
            compute_murmur2_fingerprint(b"a"),
            compute_murmur2_fingerprint(b"ab")
        );
    }
}

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{MubError, Result};

/// Default Azure AD public client ID placeholder (users can override in settings).
pub const DEFAULT_MSA_CLIENT_ID: &str = "00000000402b5328";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountRole {
    Player,
    Bot,
}

impl std::fmt::Display for AccountRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountRole::Player => write!(f, "Player"),
            AccountRole::Bot => write!(f, "Bot"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub username: String,
    pub mc_uuid: String,
    pub role: AccountRole,
    #[serde(default)]
    pub tags: Vec<String>,
    pub msa_refresh_token: Option<String>,
    pub mc_access_token: Option<String>,
    pub mc_token_expires_at: Option<DateTime<Utc>>,
    pub added_at: DateTime<Utc>,
}

impl Account {
    pub fn new_offline(username: impl Into<String>, role: AccountRole) -> Self {
        let username = username.into();
        let id = Uuid::new_v4().to_string();
        let mc_uuid = Uuid::new_v4().simple().to_string();
        Self {
            id,
            username,
            mc_uuid,
            role,
            tags: Vec::new(),
            msa_refresh_token: None,
            mc_access_token: None,
            mc_token_expires_at: None,
            added_at: Utc::now(),
        }
    }

    pub fn is_token_valid(&self) -> bool {
        match (&self.mc_access_token, self.mc_token_expires_at) {
            (Some(_), Some(exp)) => exp > Utc::now() + chrono::Duration::minutes(5),
            _ => false,
        }
    }

    pub fn is_msa(&self) -> bool {
        self.msa_refresh_token.is_some()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AccountStore {
    pub accounts: Vec<Account>,
}

impl AccountStore {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|a| a.id == id)
    }

    pub fn find_by_username(&self, username: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|a| a.username.eq_ignore_ascii_case(username))
    }

    pub fn upsert(&mut self, account: Account) {
        if let Some(existing) = self
            .accounts
            .iter_mut()
            .find(|a| a.mc_uuid == account.mc_uuid || a.id == account.id)
        {
            *existing = account;
        } else {
            self.accounts.push(account);
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<Account> {
        if let Some(idx) = self.accounts.iter().position(|a| a.id == id) {
            Some(self.accounts.remove(idx))
        } else {
            None
        }
    }
}

/// Response from Microsoft's OAuth2 device code endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MsaTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MsaErrorResponse {
    error: String,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XblResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XblDisplayClaims,
}

#[derive(Debug, Deserialize)]
struct XblDisplayClaims {
    xui: Vec<XblXui>,
}

#[derive(Debug, Deserialize)]
struct XblXui {
    uhs: String,
}

#[derive(Debug, Deserialize)]
struct McAuthResponse {
    access_token: String,
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct McProfileResponse {
    id: String,
    name: String,
}

/// Handles Microsoft -> Xbox Live -> XSTS -> Minecraft Services authentication.
pub struct MsaAuthClient {
    http: Client,
    client_id: String,
}

const LIVE_REDIRECT_URI: &str = "https://login.live.com/oauth20_desktop.srf";
const LIVE_SCOPE: &str = "XboxLive.signin offline_access";

impl MsaAuthClient {
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            client_id: client_id.into(),
        }
    }

    pub fn uses_live_flow(&self) -> bool {
        self.client_id == DEFAULT_MSA_CLIENT_ID
    }

    /// Browser authorization URL for the default Minecraft client ID (`login.live.com`).
    pub fn live_authorize_url() -> String {
        format!(
            "https://login.live.com/oauth20_authorize.srf?client_id={DEFAULT_MSA_CLIENT_ID}&response_type=code&redirect_uri=https%3A%2F%2Flogin.live.com%2Foauth20_desktop.srf&scope=XboxLive.signin%20offline_access"
        )
    }

    /// Exchange a pasted redirect URL (`https://login.live.com/oauth20_desktop.srf?code=...`)
    /// or raw authorization code for a Minecraft account.
    pub async fn exchange_live_code(
        &self,
        code_or_url: &str,
        role: AccountRole,
    ) -> Result<Account> {
        let code = extract_auth_code(code_or_url).ok_or_else(|| {
            MubError::Auth(
                "Could not find '?code=...' in the pasted URL or code string.".into(),
            )
        })?;

        let resp = self
            .http
            .post("https://login.live.com/oauth20_token.srf")
            .form(&[
                ("client_id", DEFAULT_MSA_CLIENT_ID),
                ("code", code.as_str()),
                ("grant_type", "authorization_code"),
                ("redirect_uri", LIVE_REDIRECT_URI),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(MubError::Auth(format!(
                "Live token exchange failed: {text}"
            )));
        }

        let msa: MsaTokenResponse = resp.json().await?;
        self.complete_mc_auth(&msa.access_token, msa.refresh_token, role, None, true)
            .await
    }

    /// Step 1: Request a device code for the user to sign in via browser.
    pub async fn request_device_code(&self) -> Result<DeviceCodeResponse> {
        let resp = self
            .http
            .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("scope", "XboxLive.signin offline_access"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(MubError::Auth(format!("Device code request failed: {text}")));
        }

        Ok(resp.json().await?)
    }

    /// Step 2: Poll until the user completes authentication in their browser.
    pub async fn poll_device_code(
        &self,
        device: &DeviceCodeResponse,
        role: AccountRole,
    ) -> Result<Account> {
        let deadline = std::time::Instant::now() + Duration::from_secs(device.expires_in);
        let interval = Duration::from_secs(device.interval.max(2));

        loop {
            if std::time::Instant::now() >= deadline {
                return Err(MubError::Auth("Device code expired before sign-in completed".into()));
            }

            tokio::time::sleep(interval).await;

            let resp = self
                .http
                .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
                .form(&[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", self.client_id.as_str()),
                    ("device_code", device.device_code.as_str()),
                ])
                .send()
                .await?;

            if resp.status().is_success() {
                let msa: MsaTokenResponse = resp.json().await?;
                return self
                    .complete_mc_auth(&msa.access_token, msa.refresh_token, role, None, true)
                    .await;
            }

            let err: MsaErrorResponse = resp.json().await?;
            match err.error.as_str() {
                "authorization_pending" | "slow_down" => continue,
                _ => {
                    return Err(MubError::Auth(format!(
                        "MSA token polling failed ({}): {}",
                        err.error,
                        err.error_description.unwrap_or_default()
                    )));
                }
            }
        }
    }

    /// Refresh an existing MSA account's Minecraft access token using its refresh token.
    pub async fn refresh_account(&self, account: &Account) -> Result<Account> {
        let refresh_token = account
            .msa_refresh_token
            .as_deref()
            .ok_or_else(|| MubError::Auth("Account has no MSA refresh token".into()))?;

        let (token_url, prefix_d, form) = if self.uses_live_flow() {
            (
                "https://login.live.com/oauth20_token.srf",
                true,
                vec![
                    ("client_id", DEFAULT_MSA_CLIENT_ID),
                    ("refresh_token", refresh_token),
                    ("grant_type", "refresh_token"),
                    ("redirect_uri", LIVE_REDIRECT_URI),
                    ("scope", LIVE_SCOPE),
                ],
            )
        } else {
            (
                "https://login.microsoftonline.com/consumers/oauth2/v2.0/token",
                true,
                vec![
                    ("grant_type", "refresh_token"),
                    ("client_id", self.client_id.as_str()),
                    ("refresh_token", refresh_token),
                    ("scope", "XboxLive.signin offline_access"),
                ],
            )
        };

        let resp = self.http.post(token_url).form(&form).send().await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(MubError::Auth(format!("MSA refresh failed: {text}")));
        }

        let msa: MsaTokenResponse = resp.json().await?;
        let next_refresh = msa
            .refresh_token
            .or_else(|| account.msa_refresh_token.clone());

        self.complete_mc_auth(
            &msa.access_token,
            next_refresh,
            account.role,
            Some(account),
            prefix_d,
        )
        .await
    }

    async fn complete_mc_auth(
        &self,
        msa_access_token: &str,
        msa_refresh_token: Option<String>,
        role: AccountRole,
        existing: Option<&Account>,
        prefix_d: bool,
    ) -> Result<Account> {
        let rps_ticket = if prefix_d {
            format!("d={msa_access_token}")
        } else {
            msa_access_token.to_string()
        };
        // 1. Xbox Live
        let xbl_resp = self
            .http
            .post("https://user.auth.xboxlive.com/user/authenticate")
            .json(&serde_json::json!({
                "Properties": {
                    "AuthMethod": "RPS",
                    "SiteName": "user.auth.xboxlive.com",
                    "RpsTicket": rps_ticket
                },
                "RelyingParty": "http://auth.xboxlive.com",
                "TokenType": "JWT"
            }))
            .send()
            .await?;

        if !xbl_resp.status().is_success() {
            let text = xbl_resp.text().await.unwrap_or_default();
            return Err(MubError::Auth(format!("Xbox Live auth failed: {text}")));
        }
        let xbl: XblResponse = xbl_resp.json().await?;
        let uhs = xbl
            .display_claims
            .xui
            .first()
            .map(|x| x.uhs.clone())
            .ok_or_else(|| MubError::Auth("Missing Xbox user hash (uhs)".into()))?;

        // 2. XSTS
        let xsts_resp = self
            .http
            .post("https://xsts.auth.xboxlive.com/xsts/authorize")
            .json(&serde_json::json!({
                "Properties": {
                    "SandboxId": "RETAIL",
                    "UserTokens": [xbl.token]
                },
                "RelyingParty": "rp://api.minecraftservices.com/",
                "TokenType": "JWT"
            }))
            .send()
            .await?;

        if !xsts_resp.status().is_success() {
            let text = xsts_resp.text().await.unwrap_or_default();
            return Err(MubError::Auth(format!("XSTS auth failed: {text}")));
        }
        let xsts: XblResponse = xsts_resp.json().await?;

        // 3. Minecraft Services login_with_xbox
        let mc_resp = self
            .http
            .post("https://api.minecraftservices.com/authentication/login_with_xbox")
            .json(&serde_json::json!({
                "identityToken": format!("XBL3.0 x={uhs};{}", xsts.token)
            }))
            .send()
            .await?;

        if !mc_resp.status().is_success() {
            let text = mc_resp.text().await.unwrap_or_default();
            return Err(MubError::Auth(format!("Minecraft login_with_xbox failed: {text}")));
        }
        let mc_auth: McAuthResponse = mc_resp.json().await?;

        // 4. Fetch Minecraft Profile (UUID + Username)
        let profile_resp = self
            .http
            .get("https://api.minecraftservices.com/minecraft/profile")
            .bearer_auth(&mc_auth.access_token)
            .send()
            .await?;

        if !profile_resp.status().is_success() {
            let text = profile_resp.text().await.unwrap_or_default();
            return Err(MubError::Auth(format!(
                "Failed to fetch Minecraft profile (does this account own Minecraft?): {text}"
            )));
        }
        let profile: McProfileResponse = profile_resp.json().await?;

        Ok(Account {
            id: existing
                .map(|a| a.id.clone())
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            username: profile.name,
            mc_uuid: profile.id,
            role,
            tags: existing.map(|a| a.tags.clone()).unwrap_or_default(),
            msa_refresh_token,
            mc_access_token: Some(mc_auth.access_token),
            mc_token_expires_at: Some(Utc::now() + chrono::Duration::seconds(mc_auth.expires_in)),
            added_at: existing.map(|a| a.added_at).unwrap_or_else(Utc::now),
        })
    }
}

fn extract_auth_code(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let raw = if let Some(idx) = trimmed.find("code=") {
        let rest = &trimmed[idx + 5..];
        rest.split(['&', '#']).next().unwrap_or(rest).trim()
    } else if !trimmed.contains("://") {
        trimmed
    } else {
        return None;
    };
    if raw.is_empty() {
        return None;
    }
    Some(percent_decode(raw))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(val) = u8::from_str_radix(hex_str, 16)
        {
            out.push(val);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

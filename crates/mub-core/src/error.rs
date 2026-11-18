use thiserror::Error;

#[derive(Debug, Error)]
pub enum MubError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Zip extraction error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Profile not found: {0}")]
    ProfileNotFound(String),

    #[error("Instance already running: {0}")]
    InstanceAlreadyRunning(String),

    #[error("Instance not running: {0}")]
    InstanceNotRunning(String),

    #[error("Download / verification error: {0}")]
    Download(String),

    #[error("Launcher error: {0}")]
    Launch(String),
}

pub type Result<T> = std::result::Result<T, MubError>;

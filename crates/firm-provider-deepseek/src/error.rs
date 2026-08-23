#[derive(Debug, thiserror::Error)]
pub enum DeepSeekError {
    #[error("{0}")]
    Usage(String),
    #[error("application callback failed: {detail}")]
    Callback {
        detail: String,
        supervisor_lease_lost: bool,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type DeepSeekResult<T> = Result<T, DeepSeekError>;

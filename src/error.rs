/// Errors that can arise in the operation of the session stores
/// included in this crate
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// an error that comes from sqlx
    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),

    /// an error that comes from serde_json
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),

    /// an error that comes from base64
    #[error(transparent)]
    Base64(#[from] base64::DecodeError),
}

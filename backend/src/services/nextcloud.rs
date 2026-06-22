//! Nextcloud WebDAV upload via public share links.
//!
//! Nextcloud public-share WebDAV endpoint:
//!   PUT <base>/public.php/webdav/<filename>
//!   Authorization: Basic base64("<token>:<password>")
//! where <base> is everything before `/s/` in the share URL (preserving
//! schema + host + any sub-path like `/nextcloud`).

use crate::error::{AppError, AppResult};
use reqwest::header::CONTENT_TYPE;

/// Parse a Nextcloud public share URL into (base, token).
///
/// Example: `https://cloud.example.com/s/AbCdEf` → `("https://cloud.example.com", "AbCdEf")`
/// Sub-path installations:
///   `https://example.com/nextcloud/s/AbCdEf` → `("https://example.com/nextcloud", "AbCdEf")`
///
/// Only `https://` URLs are accepted.
pub fn parse_share_url(url: &str) -> AppResult<(String, String)> {
    let url = url.trim();
    if !url.starts_with("https://") {
        return Err(AppError::BadRequest(
            "Nextcloud share URL must start with https://".into(),
        ));
    }
    // Split at `/s/` — everything before is the base, everything after is the token.
    let sep = "/s/";
    let pos = url.find(sep).ok_or_else(|| {
        AppError::BadRequest(
            "Nextcloud share URL must contain /s/<token> \
             (e.g. https://cloud.example.com/s/AbCdEf)."
                .into(),
        )
    })?;
    let base = url[..pos].to_string();
    let after = &url[pos + sep.len()..];
    // Token is the first path segment after /s/.
    let token = after.split('/').next().unwrap_or("").trim().to_string();
    if token.is_empty() {
        return Err(AppError::BadRequest(
            "Nextcloud share URL has an empty token after /s/.".into(),
        ));
    }
    Ok((base, token))
}

/// Upload bytes to a Nextcloud public share folder via WebDAV PUT.
///
/// `base`     – everything before `/s/` in the share URL
/// `token`    – share token (segment after `/s/`)
/// `password` – optional share password (`None` or `""` = no password)
/// `filename` – filename to create inside the shared folder
/// `bytes`    – file contents
pub async fn upload_file(
    base: &str,
    token: &str,
    password: Option<&str>,
    filename: &str,
    bytes: Vec<u8>,
) -> AppResult<()> {
    let url = format!("{}/public.php/webdav/{}", base, filename);
    let pw = password.filter(|p| !p.is_empty());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| AppError::Internal(format!("failed to build HTTP client: {e}")))?;

    // reqwest::RequestBuilder::basic_auth encodes "token:password" as Basic auth.
    let response = client
        .put(&url)
        .basic_auth(token, pw)
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(bytes)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Nextcloud upload request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Nextcloud upload failed (HTTP {status}): {body}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_share_url_extracts_base_and_token() {
        let (base, token) =
            parse_share_url("https://cloud.example.com/s/AbCdEfGhIj").unwrap();
        assert_eq!(base, "https://cloud.example.com");
        assert_eq!(token, "AbCdEfGhIj");
    }

    #[test]
    fn parse_share_url_preserves_subpath() {
        let (base, token) =
            parse_share_url("https://example.com/nextcloud/s/MyToken123").unwrap();
        assert_eq!(base, "https://example.com/nextcloud");
        assert_eq!(token, "MyToken123");
    }

    #[test]
    fn parse_share_url_strips_trailing_path_from_token() {
        let (base, token) =
            parse_share_url("https://cloud.example.com/s/AbCdEf/download").unwrap();
        assert_eq!(base, "https://cloud.example.com");
        assert_eq!(token, "AbCdEf");
    }

    #[test]
    fn parse_share_url_rejects_http() {
        assert!(parse_share_url("http://cloud.example.com/s/Token").is_err());
    }

    #[test]
    fn parse_share_url_rejects_missing_s_segment() {
        assert!(parse_share_url("https://cloud.example.com/share/Token").is_err());
    }

    #[test]
    fn parse_share_url_rejects_empty_token() {
        assert!(parse_share_url("https://cloud.example.com/s/").is_err());
    }

    #[test]
    fn parse_share_url_trims_whitespace() {
        let (base, token) =
            parse_share_url("  https://cloud.example.com/s/Tok  ").unwrap();
        assert_eq!(base, "https://cloud.example.com");
        assert_eq!(token, "Tok");
    }
}

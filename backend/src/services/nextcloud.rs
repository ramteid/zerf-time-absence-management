//! Nextcloud WebDAV upload via public share links.
//!
//! Nextcloud public-share WebDAV endpoint:
//!   PUT <base>/public.php/webdav/<filename>
//!   Authorization: Basic base64("<token>:<password>")
//! where <base> is everything before `/s/` in the share URL (preserving
//! schema + host + any sub-path like `/nextcloud`).

use crate::error::{AppError, AppResult};
use reqwest::header::CONTENT_TYPE;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

/// SSRF guard: returns `true` when an IP must never be the target of an
/// outbound upload — loopback, private (RFC1918), link-local (which includes
/// the 169.254.169.254 cloud-metadata endpoint), CGNAT shared space,
/// broadcast, documentation, and the unspecified address, plus the IPv6
/// equivalents (loopback, unspecified, multicast, unique-local `fc00::/7`,
/// link-local `fe80::/10`, and the IPv4-mapped form of any of the above).
fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                // 100.64.0.0/10 — carrier-grade NAT shared address space.
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_ip(IpAddr::V4(mapped));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7 — unique local addresses.
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fe80::/10 — link-local unicast.
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Resolve the host of an outbound Nextcloud URL and reject it when any
/// resolved address is private/reserved (SSRF defence). Returns the host plus
/// the validated socket addresses, which the caller pins into the HTTP client
/// so the connection cannot be re-pointed at an internal address between this
/// check and the request (DNS-rebinding defence).
async fn resolve_safe_addrs(url: &str) -> AppResult<(String, Vec<SocketAddr>)> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| AppError::BadRequest("Invalid Nextcloud URL.".into()))?;
    if parsed.scheme() != "https" {
        return Err(AppError::BadRequest("Nextcloud URL must use https.".into()));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::BadRequest("Nextcloud URL has no host.".into()))?
        .to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|e| AppError::Internal(format!("Could not resolve Nextcloud host: {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(AppError::BadRequest(
            "Nextcloud host did not resolve to any address.".into(),
        ));
    }
    if addrs.iter().any(|addr| is_blocked_ip(addr.ip())) {
        return Err(AppError::BadRequest(
            "Nextcloud host resolves to a private or reserved address; refusing to connect."
                .into(),
        ));
    }
    Ok((host, addrs))
}

/// Build a one-off HTTP client that connects only to the pre-validated
/// addresses for `host`, closing the DNS-rebinding window opened by
/// [`resolve_safe_addrs`]. Uploads run at most daily/monthly, so building a
/// client per request is negligible.
fn pinned_client(
    host: &str,
    addrs: &[SocketAddr],
    timeout: Duration,
) -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .resolve_to_addrs(host, addrs)
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to build Nextcloud HTTP client: {e}")))
}

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

/// Create a folder in a Nextcloud public share via WebDAV MKCOL.
///
/// `folder` is a relative path inside the shared folder (no leading slash).
/// HTTP 405 (Method Not Allowed) is treated as success — it means the folder
/// already exists, which is fine since write-only shares cannot use PROPFIND.
pub async fn create_folder(
    base: &str,
    token: &str,
    password: Option<&str>,
    folder: &str,
) -> AppResult<()> {
    // Trailing slash is required by the WebDAV spec for MKCOL.
    let url = format!("{}/public.php/webdav/{}/", base, folder);
    let pw = password.filter(|p| !p.is_empty());
    let (host, addrs) = resolve_safe_addrs(&url).await?;

    let response = pinned_client(&host, &addrs, Duration::from_secs(30))?
        // reqwest does not expose MKCOL as a named method, so use from_bytes.
        .request(
            reqwest::Method::from_bytes(b"MKCOL").expect("MKCOL is a valid HTTP method"),
            &url,
        )
        .basic_auth(token, pw)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Nextcloud MKCOL request failed: {e}")))?;

    let status = response.status();
    if status.is_success() || status.as_u16() == 405 {
        // 201 Created or 405 Method Not Allowed (folder already exists).
        return Ok(());
    }
    // Any other status (e.g. 403 on strict file-drop shares) is logged as a
    // warning, not an error.  The subsequent PUT is the authoritative signal:
    // if the folder was missing, the PUT will fail with its own clear error.
    let body = response.text().await.unwrap_or_default();
    tracing::warn!("Nextcloud MKCOL returned {status} for {url} — attempting PUT anyway: {body}");
    Ok(())
}

/// Upload bytes to a Nextcloud public share via WebDAV PUT.
///
/// `base`     – everything before `/s/` in the share URL
/// `token`    – share token (segment after `/s/`)
/// `password` – optional share password (`None` or `""` = no password)
/// `path`     – relative path inside the shared folder, may include subfolders
///               (e.g. `"2026-05/2026-05_Smith_John.pdf"`)
/// `bytes`    – file contents
pub async fn upload_file(
    base: &str,
    token: &str,
    password: Option<&str>,
    path: &str,
    bytes: Vec<u8>,
) -> AppResult<()> {
    let url = format!("{}/public.php/webdav/{}", base, path);
    let pw = password.filter(|p| !p.is_empty());
    let (host, addrs) = resolve_safe_addrs(&url).await?;

    // reqwest::RequestBuilder::basic_auth encodes "token:password" as Basic auth.
    let response = pinned_client(&host, &addrs, Duration::from_secs(120))?
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

    #[test]
    fn is_blocked_ip_rejects_private_and_metadata_addresses() {
        use std::net::{Ipv4Addr, Ipv6Addr};
        let blocked = [
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),       // loopback
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),        // RFC1918
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),    // RFC1918
            IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1)),      // RFC1918
            IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)), // cloud metadata (link-local)
            IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),      // CGNAT shared
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),         // unspecified
            IpAddr::V6(Ipv6Addr::LOCALHOST),               // ::1
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),             // ::
            IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1)), // ULA fc00::/7
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)), // link-local
            // IPv4-mapped loopback must also be blocked.
            IpAddr::V6(Ipv4Addr::new(127, 0, 0, 1).to_ipv6_mapped()),
        ];
        for ip in blocked {
            assert!(is_blocked_ip(ip), "{ip} should be blocked");
        }
    }

    #[test]
    fn is_blocked_ip_allows_public_addresses() {
        use std::net::{Ipv4Addr, Ipv6Addr};
        let allowed = [
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), // example.com
            IpAddr::V6(Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111)), // public v6
        ];
        for ip in allowed {
            assert!(!is_blocked_ip(ip), "{ip} should be allowed");
        }
    }
}

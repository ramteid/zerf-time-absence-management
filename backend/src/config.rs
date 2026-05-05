use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub session_secret: String,
    pub admin_email: String,

    pub bind: String,
    pub static_dir: String,
    pub public_url: Option<String>,
    pub allowed_origins: Vec<String>,
    pub secure_cookies: bool,
    pub enforce_origin: bool,
    pub enforce_csrf: bool,
    pub trust_proxy: bool,

    /// Optional SMTP settings for outbound notification emails.
    /// All fields are optional; when `smtp.is_some()` returns false
    /// the system falls back to in-app-only notifications.
    pub smtp: Option<SmtpConfig>,
}

#[derive(Clone, Debug)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
    /// `starttls`, `tls`, or `none`. Defaults to `starttls`.
    pub encryption: String,
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(v) => matches!(
            v.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn env_opt(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl Config {
    pub fn from_env() -> Self {
        let database_url = env::var("ZERF_DATABASE_URL").expect("ZERF_DATABASE_URL must be set");
        let session_secret = env::var("ZERF_SESSION_SECRET")
            .expect("ZERF_SESSION_SECRET must be set; generate one with: openssl rand -hex 32");
        if session_secret.len() < 32 {
            panic!("ZERF_SESSION_SECRET must be at least 32 characters");
        }
        if session_secret.contains("please-change") || session_secret == "change-me" {
            panic!("ZERF_SESSION_SECRET is using a default/placeholder value — replace it with a real random secret");
        }

        let admin_email =
            env::var("ZERF_ADMIN_EMAIL").unwrap_or_else(|_| "admin@example.com".into());
        let public_url = env::var("ZERF_PUBLIC_URL").ok().filter(|s| !s.is_empty());
        let allowed_origins: Vec<String> = match env::var("ZERF_ALLOWED_ORIGINS").ok() {
            Some(s) if !s.is_empty() => s
                .split(',')
                .map(|x| x.trim().trim_end_matches('/').to_string())
                .filter(|x| !x.is_empty())
                .collect(),
            _ => public_url
                .iter()
                .map(|u| u.trim_end_matches('/').to_string())
                .collect(),
        };
        let dev_mode = env_bool("ZERF_DEV", false);
        let secure_cookies = env_bool("ZERF_SECURE_COOKIES", !dev_mode);
        let enforce_origin = env_bool("ZERF_ENFORCE_ORIGIN", !allowed_origins.is_empty());
        let enforce_csrf = env_bool("ZERF_ENFORCE_CSRF", !dev_mode);
        let trust_proxy = env_bool("ZERF_TRUST_PROXY", true);

        // SMTP is fully optional. We only build the struct when a host AND
        // a from-address are present.  Missing credentials simply mean
        // unauthenticated SMTP (test relays / local MTAs).
        let smtp = match (env_opt("ZERF_SMTP_HOST"), env_opt("ZERF_SMTP_FROM")) {
            (Some(host), Some(from)) => {
                let port: u16 = env_opt("ZERF_SMTP_PORT")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(587);
                let encryption = env_opt("ZERF_SMTP_ENCRYPTION")
                    .map(|s| s.to_lowercase())
                    .filter(|s| matches!(s.as_str(), "starttls" | "tls" | "none"))
                    .unwrap_or_else(|| "starttls".into());
                Some(SmtpConfig {
                    host,
                    port,
                    username: env_opt("ZERF_SMTP_USERNAME"),
                    password: env_opt("ZERF_SMTP_PASSWORD"),
                    from,
                    encryption,
                })
            }
            _ => None,
        };

        Self {
            database_url,
            session_secret,
            admin_email,
            bind: env::var("ZERF_BIND").unwrap_or_else(|_| "0.0.0.0:3000".into()),
            static_dir: env::var("ZERF_STATIC_DIR").unwrap_or_else(|_| "static".into()),
            public_url,
            allowed_origins,
            secure_cookies,
            enforce_origin,
            enforce_csrf,
            trust_proxy,
            smtp,
        }
    }
}

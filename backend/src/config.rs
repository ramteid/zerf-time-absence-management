use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub session_secret: String,
    pub git_commit: String,

    pub bind: String,
    pub static_dir: String,
    pub public_url: Option<String>,
    pub allowed_origins: Vec<String>,
    pub secure_cookies: bool,
    pub enforce_origin: bool,
    pub enforce_csrf: bool,
    pub trust_proxy: bool,
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
        Ok(value) => matches!(
            value.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

impl Config {
    pub fn from_env() -> Self {
        let database_url = env::var("ZERF_DATABASE_URL").expect("ZERF_DATABASE_URL must be set");
        let session_secret = env::var("ZERF_SESSION_SECRET")
            .expect("ZERF_SESSION_SECRET must be set; generate one with: openssl rand -hex 32");
        if session_secret.len() < 32 {
            panic!("ZERF_SESSION_SECRET must be at least 32 characters");
        }
        if session_secret.contains("please-change") || session_secret.contains("change-me") {
            panic!("ZERF_SESSION_SECRET is using a default/placeholder value — replace it with a real random secret");
        }

        let public_url = env::var("ZERF_PUBLIC_URL")
            .ok()
            .filter(|url| !url.is_empty());
        let allowed_origins: Vec<String> = match env::var("ZERF_ALLOWED_ORIGINS").ok() {
            Some(origins_str) if !origins_str.is_empty() => origins_str
                .split(',')
                .map(|origin| origin.trim().trim_end_matches('/').to_string())
                .filter(|origin| !origin.is_empty())
                .collect(),
            _ => public_url
                .iter()
                .map(|url| url.trim_end_matches('/').to_string())
                .collect(),
        };
        let dev_mode = env_bool("ZERF_DEV", false);
        let secure_cookies = env_bool("ZERF_SECURE_COOKIES", !dev_mode);
        let enforce_origin = env_bool("ZERF_ENFORCE_ORIGIN", !allowed_origins.is_empty());
        let enforce_csrf = env_bool("ZERF_ENFORCE_CSRF", !dev_mode);
        let trust_proxy = env_bool("ZERF_TRUST_PROXY", true);

        Self {
            database_url,
            session_secret,
            git_commit: env::var("ZERF_GIT_COMMIT").unwrap_or_else(|_| "unknown".into()),
            bind: env::var("ZERF_BIND").unwrap_or_else(|_| "0.0.0.0:3333".into()),
            static_dir: env::var("ZERF_STATIC_DIR").unwrap_or_else(|_| "static".into()),
            public_url,
            allowed_origins,
            secure_cookies,
            enforce_origin,
            enforce_csrf,
            trust_proxy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_env<F: FnOnce()>(overrides: &[(&str, Option<&str>)], test: F) {
        let _guard = env_lock().lock().expect("env lock poisoned");
        let keys = [
            "ZERF_DATABASE_URL",
            "ZERF_SESSION_SECRET",
            "ZERF_GIT_COMMIT",
            "ZERF_BIND",
            "ZERF_STATIC_DIR",
            "ZERF_PUBLIC_URL",
            "ZERF_ALLOWED_ORIGINS",
            "ZERF_DEV",
            "ZERF_SECURE_COOKIES",
            "ZERF_ENFORCE_ORIGIN",
            "ZERF_ENFORCE_CSRF",
            "ZERF_TRUST_PROXY",
        ];
        let snapshot: Vec<(String, Option<String>)> = keys
            .iter()
            .map(|k| (k.to_string(), std::env::var(k).ok()))
            .collect();

        for (key, value) in overrides {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }

        test();

        for (key, value) in snapshot {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }

    #[test]
    fn env_bool_parses_truthy_values_and_defaults_falsey() {
        with_env(&[("ZERF_DEV", Some("YES"))], || {
            assert!(env_bool("ZERF_DEV", false));
        });
        with_env(&[("ZERF_DEV", Some("off"))], || {
            assert!(!env_bool("ZERF_DEV", true));
        });
        with_env(&[("ZERF_DEV", None)], || {
            assert!(env_bool("ZERF_DEV", true));
            assert!(!env_bool("ZERF_DEV", false));
        });
    }

    #[test]
    fn from_env_uses_public_url_as_default_allowed_origin() {
        with_env(
            &[
                ("ZERF_DATABASE_URL", Some("postgres://localhost/db")),
                (
                    "ZERF_SESSION_SECRET",
                    Some("01234567890123456789012345678901"),
                ),
                ("ZERF_PUBLIC_URL", Some("https://zerf.example/")),
                ("ZERF_ALLOWED_ORIGINS", None),
                ("ZERF_DEV", Some("false")),
                ("ZERF_SECURE_COOKIES", None),
                ("ZERF_ENFORCE_CSRF", None),
                ("ZERF_ENFORCE_ORIGIN", None),
            ],
            || {
                let cfg = Config::from_env();
                assert_eq!(cfg.public_url.as_deref(), Some("https://zerf.example/"));
                assert_eq!(cfg.allowed_origins, vec!["https://zerf.example"]);
                assert!(cfg.secure_cookies);
                assert!(cfg.enforce_csrf);
                assert!(cfg.enforce_origin);
            },
        );
    }

    #[test]
    fn from_env_parses_allowed_origins_and_dev_defaults() {
        with_env(
            &[
                ("ZERF_DATABASE_URL", Some("postgres://localhost/db")),
                (
                    "ZERF_SESSION_SECRET",
                    Some("01234567890123456789012345678901"),
                ),
                ("ZERF_PUBLIC_URL", Some("https://public.example")),
                (
                    "ZERF_ALLOWED_ORIGINS",
                    Some(" https://a.example/,https://b.example ,,https://c.example/ "),
                ),
                ("ZERF_DEV", Some("true")),
                ("ZERF_SECURE_COOKIES", None),
                ("ZERF_ENFORCE_CSRF", None),
                ("ZERF_ENFORCE_ORIGIN", None),
            ],
            || {
                let cfg = Config::from_env();
                assert_eq!(
                    cfg.allowed_origins,
                    vec![
                        "https://a.example",
                        "https://b.example",
                        "https://c.example"
                    ]
                );
                assert!(!cfg.secure_cookies);
                assert!(!cfg.enforce_csrf);
                assert!(cfg.enforce_origin);
            },
        );
    }
}

use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_path: String,
    pub session_secret: String,
    pub admin_email: String,
    pub organization_name: String,
    pub region: String,
    pub bind: String,
    pub static_dir: String,
    pub public_url: Option<String>,
    pub allowed_origins: Vec<String>,
    pub secure_cookies: bool,
    pub enforce_origin: bool,
    pub enforce_csrf: bool,
    pub trust_proxy: bool,
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(v) => matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

impl Config {
    pub fn from_env() -> Self {
        let database_path = env::var("KITAZEIT_DATABASE_PATH").unwrap_or_else(|_| "data/kitazeit.db".into());
        let session_secret = env::var("KITAZEIT_SESSION_SECRET")
            .expect("KITAZEIT_SESSION_SECRET must be set; generate one with: openssl rand -hex 32");
        if session_secret.len() < 32 {
            panic!("KITAZEIT_SESSION_SECRET must be at least 32 characters");
        }
        if session_secret.contains("please-change") || session_secret == "change-me" {
            panic!("KITAZEIT_SESSION_SECRET is using a default/placeholder value — replace it with a real random secret");
        }

        let admin_email = env::var("KITAZEIT_ADMIN_EMAIL").unwrap_or_else(|_| "admin@example.com".into());
        let public_url = env::var("KITAZEIT_PUBLIC_URL").ok().filter(|s| !s.is_empty());
        let allowed_origins: Vec<String> = match env::var("KITAZEIT_ALLOWED_ORIGINS").ok() {
            Some(s) if !s.is_empty() => s.split(',').map(|x| x.trim().trim_end_matches('/').to_string()).filter(|x| !x.is_empty()).collect(),
            _ => public_url.iter().map(|u| u.trim_end_matches('/').to_string()).collect(),
        };
        // Default secure-by-default in production; opt-out only when explicitly set.
        let dev_mode = env_bool("KITAZEIT_DEV", false);
        let secure_cookies = env_bool("KITAZEIT_SECURE_COOKIES", !dev_mode);
        let enforce_origin = env_bool("KITAZEIT_ENFORCE_ORIGIN", !allowed_origins.is_empty());
        let enforce_csrf = env_bool("KITAZEIT_ENFORCE_CSRF", !dev_mode);
        let trust_proxy = env_bool("KITAZEIT_TRUST_PROXY", true);

        Self {
            database_path,
            session_secret,
            admin_email,
            organization_name: env::var("KITAZEIT_ORGANIZATION_NAME").unwrap_or_else(|_| "Kindergarten".into()),
            region: env::var("KITAZEIT_REGION").unwrap_or_else(|_| "BW".into()),
            bind: env::var("KITAZEIT_BIND").unwrap_or_else(|_| "0.0.0.0:3000".into()),
            static_dir: env::var("KITAZEIT_STATIC_DIR").unwrap_or_else(|_| "static".into()),
            public_url,
            allowed_origins,
            secure_cookies,
            enforce_origin,
            enforce_csrf,
            trust_proxy,
        }
    }
}

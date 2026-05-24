use reqwest::{Client, StatusCode};
use serde_json::Value;

/// A cookie-jar-equipped HTTP client that targets a specific [`TestApp`].
/// Each `TestClient` maintains its own session (like a separate browser).
pub struct TestClient {
    client: Client,
    base_url: String,
}

impl TestClient {
    pub fn new(base_url: &str) -> Self {
        let client = Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: base_url.to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// GET request, returns (status, body as Value).
    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        let resp = self
            .client
            .get(self.url(path))
            .send()
            .await
            .expect("GET failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, body)
    }

    /// GET request, returns (status, raw body string).
    pub async fn get_raw(&self, path: &str) -> (StatusCode, String) {
        let resp = self
            .client
            .get(self.url(path))
            .send()
            .await
            .expect("GET failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        (status, text)
    }

    /// POST with JSON body.
    pub async fn post(&self, path: &str, json: &Value) -> (StatusCode, Value) {
        let resp = self
            .client
            .post(self.url(path))
            .json(json)
            .send()
            .await
            .expect("POST failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, body)
    }

    /// POST with raw string body (for malformed JSON tests).
    pub async fn post_raw(&self, path: &str, body: &str) -> (StatusCode, String) {
        let resp = self
            .client
            .post(self.url(path))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .expect("POST raw failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        (status, text)
    }

    /// PUT with JSON body.
    pub async fn put(&self, path: &str, json: &Value) -> (StatusCode, Value) {
        let resp = self
            .client
            .put(self.url(path))
            .json(json)
            .send()
            .await
            .expect("PUT failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, body)
    }

    /// DELETE request.
    pub async fn delete(&self, path: &str) -> (StatusCode, Value) {
        let resp = self
            .client
            .delete(self.url(path))
            .send()
            .await
            .expect("DELETE failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, body)
    }

    // ----- Convenience helpers -----

    /// Login and return the response body.
    pub async fn login(&self, email: &str, password: &str) -> (StatusCode, Value) {
        self.post(
            "/api/v1/auth/login",
            &serde_json::json!({"email": email, "password": password}),
        )
        .await
    }

    /// Change password.
    pub async fn change_password(&self, current: &str, new: &str) -> (StatusCode, Value) {
        self.put(
            "/api/v1/auth/password",
            &serde_json::json!({"current_password": current, "new_password": new}),
        )
        .await
    }
}

//! The one HTTP endpoint of a Gradido community: GraphQL over POST.
//!
//! There is no socket and no push. The session is a short-lived JWT; every
//! authorized answer carries a fresh one in the `token` response header, and
//! taking it each time is what keeps the program signed in.

pub mod documents;
pub mod types;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// `403.13`: the token is past its time. Only a fresh one from the browser helps.
    SessionExpired,
    /// The server said no; the text is its `CODE: REASON`.
    Refused(String),
    /// Nothing came back, or not what a Gradido server sends.
    Network(String),
}

impl ApiError {
    /// The `REASON` of a `CODE: REASON` refusal.
    pub fn reason(&self) -> &str {
        match self {
            ApiError::Refused(message) => message.rsplit(": ").next().unwrap_or(message),
            _ => "",
        }
    }

    pub fn is(&self, code: &str) -> bool {
        matches!(self, ApiError::Refused(message) if message.starts_with(code))
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

// TEMPORARY, until answering a message (`replyTo`) is released: the release
// v3.1.0 does not know it and refuses every document that names it. Remove
// this constant, `Client::replies`, `Client::ask_version`, `without_replies`
// and what uses them once the release that has the feature is out.
const COMMIT_WITHOUT_REPLIES: &str = "73a0355ddf1469eba07c756d5d1759b8b20feae7";

#[derive(Clone)]
pub struct Client {
    url: String,
    agent: ureq::Agent,
    token: Arc<Mutex<String>>,
    /// TEMPORARY, see `COMMIT_WITHOUT_REPLIES`.
    replies: Arc<AtomicBool>,
}

/// TEMPORARY, see `COMMIT_WITHOUT_REPLIES`: a document as a server without `replyTo` takes it.
fn without_replies(document: &str) -> String {
    const SELECTION: &str = " replyTo {";
    let mut document = document.replace(", $replyTo: String", "").replace(", replyTo: $replyTo", "");
    if let Some(start) = document.find(SELECTION) {
        let mut depth = 0;
        for (at, letter) in document[start..].char_indices() {
            match letter {
                '{' => depth += 1,
                '}' if depth == 1 => {
                    document.replace_range(start..start + at + 1, "");
                    break;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
    }
    document
}

/// `https://host`, `host/` and `https://host/graphql` all mean the same endpoint.
pub fn endpoint(entered: &str) -> String {
    let mut url = entered.trim().trim_end_matches('/').to_string();
    if !url.contains("://") {
        url = format!("https://{url}");
    }
    if !url.ends_with("/graphql") {
        url.push_str("/graphql");
    }
    url
}

impl Client {
    pub fn new(server: &str, token: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            // Refusals come as GraphQL errors, some with a 4xx: read the body either way.
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .new_agent();
        // Pasted from a console the token often comes with quotes or a "Bearer ".
        let token = token.trim().trim_matches('"').trim_start_matches("Bearer ").trim().to_string();
        Self {
            url: endpoint(server),
            agent,
            token: Arc::new(Mutex::new(token)),
            replies: Arc::new(AtomicBool::new(true)),
        }
    }

    /// TEMPORARY, see `COMMIT_WITHOUT_REPLIES`: whether the server knows `replyTo`.
    pub fn replies(&self) -> bool {
        self.replies.load(Ordering::Relaxed)
    }

    /// TEMPORARY, see `COMMIT_WITHOUT_REPLIES`: asks `GET /api/version` which build runs.
    /// No answer, or one that names another commit, leaves answering on.
    pub fn ask_version(&self) {
        let url = format!("{}/api/version", self.url.trim_end_matches("/graphql"));
        let commit = self
            .agent
            .get(&url)
            .call()
            .ok()
            .and_then(|mut response| response.body_mut().read_to_string().ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|version| version["commit"].as_str().map(str::to_string));
        self.replies.store(commit.as_deref() != Some(COMMIT_WITHOUT_REPLIES), Ordering::Relaxed);
    }

    /// The newest token, the one to keep for the next start.
    pub fn token(&self) -> String {
        self.token.lock().unwrap().clone()
    }

    /// One operation with one top-level field, whose value is returned.
    pub fn call<T: DeserializeOwned>(&self, document: &str, mut variables: Value, field: &str) -> ApiResult<T> {
        // TEMPORARY, see `COMMIT_WITHOUT_REPLIES`.
        let stripped;
        let document = if self.replies() {
            document
        } else {
            if let Some(variables) = variables.as_object_mut() {
                variables.remove("replyTo");
            }
            stripped = without_replies(document);
            &stripped
        };
        let body = json!({ "query": document, "variables": variables }).to_string();
        let offset = -chrono::Local::now().offset().local_minus_utc() / 60;
        let mut response = self
            .agent
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.token()))
            .header("clientTimezoneOffset", offset.to_string())
            .send(body.as_str())
            .map_err(|e| ApiError::Network(e.to_string()))?;
        if let Some(fresh) = response.headers().get("token").and_then(|v| v.to_str().ok())
            && !fresh.is_empty()
        {
            *self.token.lock().unwrap() = fresh.to_string();
        }
        let status = response.status();
        let text = response.body_mut().read_to_string().map_err(|e| ApiError::Network(e.to_string()))?;
        let answer: Value = serde_json::from_str(&text).map_err(|_| ApiError::Network(format!("HTTP {status}, no GraphQL answer")))?;
        if let Some(message) = answer["errors"][0]["message"].as_str() {
            return Err(if message.starts_with("403.13") {
                ApiError::SessionExpired
            } else {
                ApiError::Refused(message.to_string())
            });
        }
        serde_json::from_value(answer["data"][field].clone()).map_err(|e| ApiError::Network(format!("unexpected answer for {field}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_from_what_people_type() {
        assert_eq!(endpoint("gradido.example"), "https://gradido.example/graphql");
        assert_eq!(endpoint(" http://localhost/ "), "http://localhost/graphql");
        assert_eq!(endpoint("https://a.example/graphql"), "https://a.example/graphql");
    }

    /// TEMPORARY, see `COMMIT_WITHOUT_REPLIES`.
    #[test]
    fn documents_for_a_server_without_replies() {
        for document in [
            documents::MESSAGES_WITH_MEMBER,
            documents::NEW_MESSAGES_SINCE,
            documents::SEND_MESSAGE,
            documents::EDIT_MESSAGE,
            documents::GROUP_MESSAGES,
            documents::SEND_GROUP_MESSAGE,
        ] {
            let stripped = without_replies(document);
            assert!(!stripped.contains("replyTo"), "{stripped}");
            assert_eq!(stripped.matches('{').count(), stripped.matches('}').count());
            assert!(stripped.ends_with("forwardedFrom { communityUuid gradidoID alias } }"), "{stripped}");
        }
        assert_eq!(without_replies(documents::GROUPS), documents::GROUPS);
    }

    #[test]
    fn the_reason_of_a_refusal() {
        let refused = ApiError::Refused("CHAT_MESSAGE_NOT_SENT: TO_ONESELF".into());
        assert!(refused.is("CHAT_MESSAGE_NOT_SENT"));
        assert_eq!(refused.reason(), "TO_ONESELF");
    }
}

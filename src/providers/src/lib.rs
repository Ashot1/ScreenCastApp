//! ChatGPT sign-in protocol; native shells own browser and credential persistence.
mod endpoints;
#[cfg(unix)]
mod local_host;
#[cfg(not(unix))]
#[path = "local_host_other.rs"]
mod local_host;
pub use local_host::{LocalHost, discover};
#[cfg(unix)]
mod managed;
#[cfg(not(unix))]
#[path = "managed_other.rs"]
mod managed;
#[cfg(unix)]
mod managed_recipe;
mod managed_types;
pub use managed::{managed_install, managed_status};
pub use managed_types::{InstallResult, InstallStatus, ManagedStatus};
mod mcp;
mod model;
mod model_registry;
mod oauth;
mod oauth_registry;
mod observation;
pub use model::{run_model, run_model_step, validate_assistance_body};
pub use oauth::{
    OAuthBegin, OAuthOutcome, OAuthRevoke, OAuthStart, OAuthTokens, oauth_begin, oauth_exchange,
    oauth_refresh, oauth_revoke,
};
pub use observation::run_observation;
mod protocol;
mod transport;
mod wait;
pub use protocol::*;

pub const RESULT_LIMIT: usize = 65_536;
pub const TOKEN_LIMIT: usize = 16_384;
pub const SCOPE_LIMIT: usize = 4096;
pub const MAX_SCOPES: usize = 64;

#[derive(Clone)]
pub struct Identity {
    pub issuer: String,
    pub subject: String,
}
#[derive(Clone)]
pub struct Hint {
    pub token: String,
    pub nonce: String,
}
#[derive(Clone)]
pub struct Registration {
    pub host: String,
    pub state: u32,
    pub client_id: Option<String>,
    pub identity: Option<Identity>,
    pub hint: Option<Hint>,
}
#[derive(Clone)]
pub struct Credentials {
    pub access: String,
    pub refresh: String,
    pub scopes: Vec<String>,
    pub expires_at: i64,
}
#[derive(Clone)]
pub struct Snapshot {
    pub registration: Registration,
    pub credentials: Option<Credentials>,
}
impl Snapshot {
    pub fn validate(&self) -> Result<(), AuthError> {
        let r = &self.registration;
        let uuid = r
            .host
            .strip_prefix("urn:uuid:")
            .ok_or(AuthError::InvalidInput)?;
        if uuid.len() != 36
            || !uuid.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            })
        {
            return Err(AuthError::InvalidInput);
        }
        match r.state {
            0 | 1 if r.client_id.is_none() && r.identity.is_none() => {}
            2 if r.client_id.as_deref().is_some_and(valid_client_id) && r.identity.is_none() => {}
            3 if r.client_id.as_deref().is_some_and(valid_client_id)
                && r.identity.as_ref().is_some_and(|i| {
                    i.issuer == endpoints::ISSUER && bounded(&i.subject, 1024)
                }) => {}
            _ => return Err(AuthError::InvalidInput),
        }
        if let Some(h) = &r.hint
            && (r.state != 3 || !bounded(&h.token, TOKEN_LIMIT) || !bounded(&h.nonce, 1024))
        {
            return Err(AuthError::InvalidInput);
        }
        if let Some(c) = &self.credentials
            && (r.state != 3
                || r.hint.is_none()
                || !bounded(&c.access, TOKEN_LIMIT)
                || !bounded(&c.refresh, TOKEN_LIMIT)
                || c.expires_at <= 0
                || !valid_scopes(&c.scopes)
                || !required_scopes(&c.scopes))
        {
            return Err(AuthError::InvalidInput);
        }
        let bytes = r.host.len()
            + r.client_id.as_ref().map_or(0, String::len)
            + r.identity
                .as_ref()
                .map_or(0, |i| i.issuer.len() + i.subject.len())
            + r.hint.as_ref().map_or(0, |h| h.token.len() + h.nonce.len())
            + self.credentials.as_ref().map_or(0, |c| {
                c.access.len() + c.refresh.len() + c.scopes.iter().map(String::len).sum::<usize>()
            });
        if bytes > RESULT_LIMIT {
            return Err(AuthError::ResourceLimit);
        }
        Ok(())
    }
}
pub fn valid_client_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 512
        && s != endpoints::DYNAMIC_CLIENT_ID
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub(crate) fn bounded(s: &str, n: usize) -> bool {
    !s.is_empty() && s.len() <= n && !s.contains('\0')
}
pub(crate) fn valid_scopes(s: &[String]) -> bool {
    !s.is_empty()
        && s.len() <= MAX_SCOPES
        && s.iter().map(|s| s.len() + 1).sum::<usize>() <= SCOPE_LIMIT
        && s.iter().all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b))
        })
}
pub(crate) fn required_scopes(s: &[String]) -> bool {
    ["openid", "resource.invoke", "chatgpt.tokens.use.direct"]
        .iter()
        .all(|v| s.iter().any(|s| s == v))
}

#[cfg(test)]
mod shape_tests {
    use super::*;
    pub(crate) fn initial() -> Snapshot {
        Snapshot {
            registration: Registration {
                host: "urn:uuid:12345678-1234-4234-9234-123456789abc".into(),
                state: 0,
                client_id: None,
                identity: None,
                hint: None,
            },
            credentials: None,
        }
    }
    #[test]
    fn valid_installation_can_start_and_corruption_cannot_reset_registration() {
        let mut s = initial();
        assert_eq!(s.validate(), Ok(()));
        s.registration.state = 3;
        assert_eq!(s.validate(), Err(AuthError::InvalidInput));
        s = initial();
        s.registration.host = "urn:uuid:not-a-uuid".into();
        assert_eq!(s.validate(), Err(AuthError::InvalidInput));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum AuthError {
    InvalidInput = 1,
    InvalidState = 2,
    Cancelled = 3,
    Expired = 4,
    TransportUnknown = 5,
    InvalidResponse = 6,
    IdentityMismatch = 7,
    Denied = 8,
    ResourceLimit = 9,
    Internal = 10,
}
impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "auth failure {}", *self as u32)
    }
}
impl std::error::Error for AuthError {}

#[cfg(feature = "model-test-support")]
pub use model::{run_model_loopback, run_model_step_loopback};
#[cfg(feature = "model-test-support")]
pub use observation::run_observation_loopback;

mod github_registry;

mod integration;
mod jira_registry;
mod todoist_registry;
pub use integration::{IntegrationTransport, IntegrationTransportError};

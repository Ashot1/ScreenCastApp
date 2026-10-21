//! OAuth sign-in for a managed server (ADR-021 «Вход», ADR-022): the built-in registry by
//! `ManagedServerId`, PKCE S256, a loopback redirect owned by native and `oauth2` 5.0.0 over a
//! one-shot bounded `Transport`. Stateless: native keeps the state and verifier of the one
//! sign-in it started. Tokens are returned to native for the Keychain and never stored here.
use crate::oauth_registry::{OAuthRegistry, registry};
use crate::{AuthError, transport::Transport};
use ambient_core::ManagedServerId;
use oauth2::basic::{BasicClient, BasicErrorResponseType};
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError, Scope, TokenResponse, TokenUrl,
};
use serde::Serialize;
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

/// One token request: connect, read and total are bounded inside `Transport`.
const REQUEST: Duration = Duration::from_secs(15);

/// The start of one sign-in, `{"status":"ok",…}`, or `unavailable` in a build without the
/// built-in client.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OAuthBegin {
    #[serde(rename = "ok")]
    Start(OAuthStart),
    Unavailable,
}
/// The URL to open in the browser and the values native passes back with the callback.
#[derive(Serialize)]
pub struct OAuthStart {
    pub url: String,
    pub state: String,
    pub verifier: String,
}
#[derive(Serialize)]
pub struct OAuthTokens {
    pub access_token: String,
    /// Always present after a code exchange; after a refresh only when the provider rotated it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}
/// The answer of an exchange (`Tokens`, `Denied`, `Invalid`, `Failed`) or a refresh (`Tokens`,
/// `InvalidGrant`, `Failed`), serialized as `{"status": …}`.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OAuthOutcome {
    #[serde(rename = "ok")]
    Tokens(OAuthTokens),
    /// The user declined consent in the browser.
    Denied,
    /// The callback failed a check: state, issuer, missing code or a malformed answer.
    Invalid,
    /// The refresh token is no longer valid (expired testing grant, revoked): sign in again.
    InvalidGrant,
    /// Network, provider or build problem; nothing changes.
    Failed,
}

/// The built-in Desktop client; test builds use a fixed test client when the build has none.
pub(crate) fn client(id: ManagedServerId) -> Option<(&'static str, &'static str)> {
    let built = registry(id).client;
    #[cfg(any(test, feature = "model-test-support"))]
    let built = built.or(Some((
        "test-client.apps.googleusercontent.com",
        "test-secret",
    )));
    built
}
/// The loopback redirect native listens on: `http://127.0.0.1:<port>/` exactly.
fn redirect(text: &str) -> Option<RedirectUrl> {
    let url = reqwest::Url::parse(text).ok()?;
    let exact = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.as_str() == text;
    exact.then(|| RedirectUrl::new(text.to_owned()).ok())?
}
type Client = BasicClient<
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;
fn oauth_client(
    r: &OAuthRegistry,
    client: (&str, &str),
    redirect: Option<RedirectUrl>,
) -> Option<Client> {
    let c = BasicClient::new(ClientId::new(client.0.to_owned()))
        // Google requires the secret in the code exchange and the refresh (live check).
        .set_client_secret(ClientSecret::new(client.1.to_owned()))
        .set_auth_type(AuthType::RequestBody)
        .set_auth_uri(AuthUrl::new(r.authorize.to_owned()).ok()?)
        .set_token_uri(TokenUrl::new(r.token.to_owned()).ok()?);
    Some(match redirect {
        Some(u) => c.set_redirect_uri(u),
        None => c,
    })
}
fn transport(url: &str) -> Result<Transport, AuthError> {
    Transport::routes(
        vec![("POST".into(), url.into())],
        Arc::new(AtomicBool::new(false)),
        Instant::now() + REQUEST,
    )
}

/// `None` for a redirect other than the exact loopback form.
pub fn oauth_begin(id: ManagedServerId, redirect_uri: &str) -> Option<OAuthBegin> {
    match client(id) {
        Some(c) => begin_with(registry(id), c, redirect_uri).map(OAuthBegin::Start),
        None => redirect(redirect_uri).map(|_| OAuthBegin::Unavailable),
    }
}
fn begin_with(r: &OAuthRegistry, client: (&str, &str), redirect_uri: &str) -> Option<OAuthStart> {
    let c = oauth_client(r, client, Some(redirect(redirect_uri)?))?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut request = c
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(challenge)
        .add_scopes(r.scopes.iter().map(|s| Scope::new((*s).to_owned())));
    for (k, v) in r.extra {
        request = request.add_extra_param(*k, *v);
    }
    if let Some(resource) = r.resource {
        request = request.add_extra_param("resource", resource);
    }
    let (url, state) = request.url();
    Some(OAuthStart {
        url: url.into(),
        state: state.secret().clone(),
        verifier: verifier.secret().clone(),
    })
}

pub fn oauth_exchange(
    id: ManagedServerId,
    redirect_uri: &str,
    callback_query: &str,
    state: &str,
    verifier: &str,
) -> OAuthOutcome {
    let Some(client) = client(id) else {
        return OAuthOutcome::Failed;
    };
    exchange_with(
        registry(id),
        client,
        redirect_uri,
        callback_query,
        state,
        verifier,
    )
}
/// The callback's query: exactly one of each parameter, the state of this sign-in and the
/// registry's exact issuer (`oauth2` checks neither), then the code exchange with the verifier.
fn exchange_with(
    r: &OAuthRegistry,
    client: (&str, &str),
    redirect_uri: &str,
    callback_query: &str,
    state: &str,
    verifier: &str,
) -> OAuthOutcome {
    let mut params = std::collections::BTreeMap::new();
    for (k, v) in reqwest::Url::parse(&format!("http://127.0.0.1/?{callback_query}"))
        .map(|u| u.query_pairs().into_owned().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        if params.insert(k, v).is_some() {
            return OAuthOutcome::Invalid;
        }
    }
    if params.get("state").map(String::as_str) != Some(state) || state.is_empty() {
        return OAuthOutcome::Invalid;
    }
    if let Some(error) = params.get("error") {
        return if error == "access_denied" {
            OAuthOutcome::Denied
        } else {
            OAuthOutcome::Invalid
        };
    }
    if params.get("iss").map(String::as_str) != Some(r.issuer) {
        return OAuthOutcome::Invalid;
    }
    let (Some(code), Some(redirect)) = (params.get("code"), redirect(redirect_uri)) else {
        return OAuthOutcome::Invalid;
    };
    let (Some(c), Ok(http)) = (oauth_client(r, client, Some(redirect)), transport(r.token)) else {
        return OAuthOutcome::Failed;
    };
    let response = c
        .exchange_code(AuthorizationCode::new(code.clone()))
        .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_owned()))
        .request(&http);
    match response {
        Ok(t) => match t.refresh_token() {
            // Without a refresh token the sign-in would end in an hour: not a usable answer.
            Some(refresh) => OAuthOutcome::Tokens(OAuthTokens {
                access_token: t.access_token().secret().clone(),
                refresh_token: Some(refresh.secret().clone()),
                expires_in: t.expires_in().map_or(0, |d| d.as_secs()),
            }),
            None => OAuthOutcome::Failed,
        },
        Err(RequestTokenError::ServerResponse(_)) => OAuthOutcome::Invalid,
        Err(_) => OAuthOutcome::Failed,
    }
}

pub fn oauth_refresh(id: ManagedServerId, refresh_token: &str) -> OAuthOutcome {
    let Some(client) = client(id) else {
        return OAuthOutcome::Failed;
    };
    refresh_with(registry(id), client, refresh_token)
}
/// One refresh: `invalid_grant` means the grant is gone (native revokes the key and asks to sign
/// in again); a network or other error changes nothing. A rotated refresh token is returned.
fn refresh_with(r: &OAuthRegistry, client: (&str, &str), refresh_token: &str) -> OAuthOutcome {
    let (Some(c), Ok(http)) = (oauth_client(r, client, None), transport(r.token)) else {
        return OAuthOutcome::Failed;
    };
    match c
        .exchange_refresh_token(&RefreshToken::new(refresh_token.to_owned()))
        .request(&http)
    {
        Ok(t) => OAuthOutcome::Tokens(OAuthTokens {
            access_token: t.access_token().secret().clone(),
            refresh_token: t.refresh_token().map(|r| r.secret().clone()),
            expires_in: t.expires_in().map_or(0, |d| d.as_secs()),
        }),
        Err(RequestTokenError::ServerResponse(e))
            if *e.error() == BasicErrorResponseType::InvalidGrant =>
        {
            OAuthOutcome::InvalidGrant
        }
        Err(_) => OAuthOutcome::Failed,
    }
}

/// The answer of a revoke at sign-out or removal: `{"status":"revoked"|"failed"}`.
#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum OAuthRevoke {
    Revoked,
    /// Network or provider refusal: native has already removed its own key either way.
    Failed,
}
pub fn oauth_revoke(id: ManagedServerId, refresh_token: &str) -> OAuthRevoke {
    let Some(client) = client(id) else {
        return OAuthRevoke::Failed;
    };
    revoke_with(registry(id), client, refresh_token)
}
/// One best-effort RFC 7009 revoke of the refresh token (Google ends the whole grant with it):
/// one form POST of at most 15 s, no retry; 200 is success. Built by hand because `oauth2`
/// refuses a non-HTTPS revocation URL, which only the loopback test fixture has.
fn revoke_with(r: &OAuthRegistry, client: (&str, &str), refresh_token: &str) -> OAuthRevoke {
    let Ok(mut form) = reqwest::Url::parse("http://form.invalid/") else {
        return OAuthRevoke::Failed;
    };
    form.query_pairs_mut()
        .append_pair("token", refresh_token)
        .append_pair("token_type_hint", "refresh_token")
        .append_pair("client_id", client.0)
        .append_pair("client_secret", client.1);
    let request = oauth2::http::Request::builder()
        .method("POST")
        .uri(r.revoke)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form.query().unwrap_or_default().as_bytes().to_vec());
    let (Ok(request), Ok(http)) = (request, transport(r.revoke)) else {
        return OAuthRevoke::Failed;
    };
    match oauth2::SyncHttpClient::call(&http, request) {
        Ok(response) if response.status() == 200 => OAuthRevoke::Revoked,
        _ => OAuthRevoke::Failed,
    }
}

#[cfg(test)]
#[path = "oauth_tests.rs"]
mod tests;

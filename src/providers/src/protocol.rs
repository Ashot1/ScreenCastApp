//! One short-lived protocol operation. Persistence and browser lifetime belong to native.
use crate::*;
use openidconnect::{core::*, *};
use reqwest::Url;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(PartialEq, Eq)]
enum Phase {
    Created,
    Prepared,
    CallbackAccepted,
    Finished,
}

pub struct AuthOperation {
    snapshot: Snapshot,
    redirect: Option<RedirectUrl>,
    refresh: bool,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    phase: Phase,
    metadata: Option<CoreProviderMetadata>,
    state: Option<CsrfToken>,
    nonce: Option<Nonce>,
    verifier: Option<PkceCodeVerifier>,
    code: Option<AuthorizationCode>,
    issued: Option<String>,
}
impl AuthOperation {
    pub fn new(
        snapshot: Snapshot,
        redirect: Option<String>,
        refresh: bool,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, AuthError> {
        snapshot.validate()?;
        if (refresh && (snapshot.credentials.is_none() || redirect.is_some()))
            || (!refresh && snapshot.registration.state == 1)
        {
            return Err(AuthError::InvalidState);
        }
        let redirect = redirect
            .map(|r| {
                let url = RedirectUrl::new(r).map_err(|_| AuthError::InvalidInput)?;
                let u = url.url();
                if u.scheme() != "http"
                    || u.host_str() != Some("127.0.0.1")
                    || u.port().is_none()
                    || u.path() != "/auth/callback"
                    || u.query().is_some()
                    || u.fragment().is_some()
                    || !u.username().is_empty()
                    || u.password().is_some()
                {
                    return Err(AuthError::InvalidInput);
                }
                Ok(url)
            })
            .transpose()?;
        if !refresh && redirect.is_none() {
            return Err(AuthError::InvalidInput);
        }
        Ok(Self {
            snapshot,
            redirect,
            refresh,
            cancelled,
            deadline: Instant::now() + Duration::from_secs(300),
            phase: Phase::Created,
            metadata: None,
            state: None,
            nonce: None,
            verifier: None,
            code: None,
            issued: None,
        })
    }
    pub fn check(&self) -> Result<(), AuthError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(AuthError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(AuthError::Expired)
        } else {
            Ok(())
        }
    }
    pub fn prepare(&mut self) -> Result<Option<String>, AuthError> {
        let http = transport::Transport::new(true, self.cancelled.clone(), self.deadline)?;
        self.prepare_with(&http)
    }
    fn prepare_with(&mut self, http: &impl SyncHttpClient) -> Result<Option<String>, AuthError> {
        self.check()?;
        if self.phase != Phase::Created {
            return Err(AuthError::InvalidState);
        }
        self.phase = Phase::Finished;
        let metadata = CoreProviderMetadata::discover(
            &IssuerUrl::new(endpoints::ISSUER.into()).map_err(|_| AuthError::Internal)?,
            http,
        );
        self.check()?;
        let metadata = metadata.map_err(|_| AuthError::InvalidResponse)?;
        if metadata.issuer().as_str() != endpoints::ISSUER
            || metadata.authorization_endpoint().as_str() != endpoints::AUTHORIZE
            || metadata.token_endpoint().map(|u| u.as_str()) != Some(endpoints::TOKEN)
            || metadata.jwks_uri().as_str() != endpoints::JWKS
            || !metadata
                .id_token_signing_alg_values_supported()
                .contains(&CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256)
        {
            return Err(AuthError::InvalidResponse);
        }
        self.check()?;
        self.metadata = Some(metadata.clone());
        let url = if self.refresh {
            None
        } else {
            let client = CoreClient::from_provider_metadata(
                metadata,
                ClientId::new(
                    self.snapshot
                        .registration
                        .client_id
                        .clone()
                        .unwrap_or_else(|| endpoints::DYNAMIC_CLIENT_ID.into()),
                ),
                None,
            )
            .set_auth_type(AuthType::RequestBody)
            .set_redirect_uri(self.redirect.clone().ok_or(AuthError::InvalidState)?);
            let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
            let mut request = client
                .authorize_url(
                    CoreAuthenticationFlow::AuthorizationCode,
                    CsrfToken::new_random,
                    Nonce::new_random,
                )
                .set_pkce_challenge(challenge)
                .add_extra_param("resource", endpoints::RESOURCE)
                .add_extra_param("ext_agent_host_id", &self.snapshot.registration.host);
            for scope in endpoints::SCOPES.iter().filter(|s| **s != "openid") {
                request = request.add_scope(Scope::new((*scope).into()));
            }
            if self.snapshot.registration.client_id.is_none() {
                request = request.add_extra_param("agent_name_hint", endpoints::AGENT_NAME);
            }
            if let Some(hint) = &self.snapshot.registration.hint {
                request = request.add_extra_param("id_token_hint", &hint.token);
            }
            let (url, state, nonce) = request.url();
            if url.as_str().len() > 32768 {
                return Err(AuthError::ResourceLimit);
            }
            self.state = Some(state);
            self.nonce = Some(nonce);
            self.verifier = Some(verifier);
            Some(url.to_string())
        };
        self.phase = Phase::Prepared;
        Ok(url)
    }
    pub fn accept_callback(&mut self, raw: &str) -> Result<String, AuthError> {
        self.check()?;
        if self.refresh || self.phase != Phase::Prepared {
            return Err(AuthError::InvalidState);
        }
        if raw.len() > 8192 {
            return Err(AuthError::ResourceLimit);
        }
        let url = Url::parse(raw).map_err(|_| AuthError::InvalidInput)?;
        let expected = self.redirect.as_ref().ok_or(AuthError::InvalidState)?.url();
        if url.scheme() != expected.scheme()
            || url.host_str() != expected.host_str()
            || url.port() != expected.port()
            || url.path() != expected.path()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(AuthError::InvalidInput);
        }
        let mut q = BTreeMap::new();
        for (k, v) in url.query_pairs() {
            if q.insert(k.into_owned(), v.into_owned()).is_some() {
                return Err(AuthError::InvalidInput);
            }
        }
        if q.get("state") != self.state.as_ref().map(|s| s.secret()) {
            return Err(AuthError::InvalidInput);
        }
        if q.contains_key("error") {
            self.phase = Phase::Finished;
            return Err(AuthError::Denied);
        }
        let issued = match (&self.snapshot.registration.client_id, q.get("client_id")) {
            (Some(saved), None) => saved.clone(),
            (Some(saved), Some(id)) if saved == id => saved.clone(),
            (None, Some(id)) if valid_client_id(id) => id.clone(),
            _ => return Err(AuthError::IdentityMismatch),
        };
        let code = q
            .remove("code")
            .filter(|s| bounded(s, 8192))
            .ok_or(AuthError::InvalidInput)?;
        self.code = Some(AuthorizationCode::new(code));
        self.issued = Some(issued.clone());
        self.phase = Phase::CallbackAccepted;
        Ok(issued)
    }
    pub fn dispatch(&mut self, persisted_client: &str) -> Result<Snapshot, AuthError> {
        let http = transport::Transport::new(false, self.cancelled.clone(), self.deadline)?;
        self.dispatch_with(persisted_client, &http)
    }
    fn dispatch_with(
        &mut self,
        persisted_client: &str,
        http: &impl SyncHttpClient,
    ) -> Result<Snapshot, AuthError> {
        self.check()?;
        let issued = if self.refresh {
            self.snapshot.registration.client_id.as_ref()
        } else {
            self.issued.as_ref()
        }
        .ok_or(AuthError::InvalidState)?
        .clone();
        if self.phase
            != if self.refresh {
                Phase::Prepared
            } else {
                Phase::CallbackAccepted
            }
            || issued != persisted_client
        {
            return Err(AuthError::InvalidState);
        }
        let metadata = self.metadata.clone().ok_or(AuthError::InvalidState)?;
        let mut client =
            CoreClient::from_provider_metadata(metadata, ClientId::new(issued.clone()), None)
                .set_auth_type(AuthType::RequestBody);
        if let Some(redirect) = self.redirect.clone() {
            client = client.set_redirect_uri(redirect);
        }
        self.phase = Phase::Finished;
        // Dispatch is consumed before I/O; a lost rotating response cannot repeat this attempt.
        let response = if self.refresh {
            let token = RefreshToken::new(
                self.snapshot
                    .credentials
                    .as_ref()
                    .ok_or(AuthError::InvalidState)?
                    .refresh
                    .clone(),
            );
            client
                .exchange_refresh_token(&token)
                .map_err(|_| AuthError::InvalidState)?
                .add_extra_param("resource", endpoints::RESOURCE)
                .request(http)
        } else {
            client
                .exchange_code(self.code.take().ok_or(AuthError::InvalidState)?)
                .map_err(|_| AuthError::InvalidState)?
                .set_pkce_verifier(self.verifier.take().ok_or(AuthError::InvalidState)?)
                .add_extra_param("resource", endpoints::RESOURCE)
                .request(http)
        };
        self.check()?;
        let response = response.map_err(|e| match e {
            RequestTokenError::Request(_) => AuthError::TransportUnknown,
            _ => AuthError::InvalidResponse,
        })?;
        if response.token_type() != &CoreTokenType::Bearer
            || !bounded(response.access_token().secret(), TOKEN_LIMIT)
        {
            return Err(AuthError::InvalidResponse);
        }
        let mut registration = self.snapshot.registration.clone();
        if let Some(id) = response.id_token() {
            let verifier = client
                .id_token_verifier()
                .set_allowed_algs([CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256])
                .set_other_audience_verifier_fn(|_| true);
            let nonce = if self.refresh {
                Nonce::new(
                    registration
                        .hint
                        .as_ref()
                        .ok_or(AuthError::InvalidState)?
                        .nonce
                        .clone(),
                )
            } else {
                self.nonce.clone().ok_or(AuthError::InvalidState)?
            };
            let claims = id
                .claims(&verifier, |actual: Option<&Nonce>| {
                    if self.refresh && actual.is_none() {
                        Ok(())
                    } else {
                        (&nonce).verify(actual)
                    }
                })
                .map_err(|_| AuthError::InvalidResponse)?;
            // The SDK exposes azp but deliberately does not enforce it.
            if claims
                .authorized_party()
                .is_some_and(|p| p.as_str() != issued)
                || (claims.audiences().len() > 1 && claims.authorized_party().is_none())
            {
                return Err(AuthError::IdentityMismatch);
            }
            if !bounded(claims.subject().as_str(), 1024) {
                return Err(AuthError::InvalidResponse);
            }
            if let Some(identity) = &registration.identity
                && (identity.issuer != claims.issuer().as_str()
                    || identity.subject != claims.subject().as_str())
            {
                return Err(AuthError::IdentityMismatch);
            }
            if let Some(hash) = claims.access_token_hash() {
                let actual = AccessTokenHash::from_token(
                    response.access_token(),
                    id.signing_alg().map_err(|_| AuthError::InvalidResponse)?,
                    id.signing_key(&verifier)
                        .map_err(|_| AuthError::InvalidResponse)?,
                )
                .map_err(|_| AuthError::InvalidResponse)?;
                if *hash != actual {
                    return Err(AuthError::InvalidResponse);
                }
            }
            let hint = id.to_string();
            if !bounded(&hint, TOKEN_LIMIT) {
                return Err(AuthError::ResourceLimit);
            }
            registration.state = 3;
            registration.client_id = Some(issued);
            registration.identity = Some(Identity {
                issuer: claims.issuer().to_string(),
                subject: claims.subject().to_string(),
            });
            registration.hint = Some(Hint {
                token: hint,
                nonce: nonce.secret().clone(),
            });
        } else if !self.refresh {
            return Err(AuthError::InvalidResponse);
        }
        let scopes = response
            .scopes()
            .map(|s| s.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .unwrap_or_else(|| {
                if self.refresh {
                    self.snapshot
                        .credentials
                        .as_ref()
                        .map(|c| c.scopes.clone())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            });
        if !scopes.is_empty() && !valid_scopes(&scopes) {
            return Err(AuthError::InvalidResponse);
        }
        let ttl = response
            .expires_in()
            .filter(|d| !d.is_zero())
            .ok_or(AuthError::InvalidResponse)?
            .as_secs();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| AuthError::Internal)?
            .as_secs();
        let expires_at = now
            .checked_add(ttl)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or(AuthError::InvalidResponse)?;
        let refresh = response
            .refresh_token()
            .map(|s| s.secret())
            .filter(|s| bounded(s, TOKEN_LIMIT));
        if self.refresh && refresh.is_none() {
            return Err(AuthError::InvalidResponse);
        }
        let credentials = if required_scopes(&scopes) {
            refresh.map(|refresh| Credentials {
                access: response.access_token().secret().clone(),
                refresh: refresh.clone(),
                scopes,
                expires_at,
            })
        } else {
            None
        };
        let result = Snapshot {
            registration,
            credentials,
        };
        result.validate()?;
        self.check()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metadata(request: HttpRequest) -> Result<HttpResponse, AuthError> {
        let body = if request.uri() == endpoints::DISCOVERY {
            serde_json::json!({"issuer":endpoints::ISSUER,"authorization_endpoint":endpoints::AUTHORIZE,"token_endpoint":endpoints::TOKEN,"jwks_uri":endpoints::JWKS,"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"]}).to_string()
        } else {
            "{\"keys\":[]}".into()
        };
        openidconnect::http::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(body.into_bytes())
            .map_err(|_| AuthError::Internal)
    }
    #[test]
    fn initial_and_returning_authorizations_keep_host_and_use_fresh_pkce()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut op = AuthOperation::new(
            crate::shape_tests::initial(),
            Some("http://127.0.0.1:12345/auth/callback".into()),
            false,
            Arc::new(AtomicBool::new(false)),
        )?;
        let url = Url::parse(&op.prepare_with(&metadata)?.ok_or("url")?)?;
        let q: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            q.get("client_id").map(String::as_str),
            Some("dynamic_agent_client")
        );
        assert_eq!(
            q.get("ext_agent_host_id").map(String::as_str),
            Some("urn:uuid:12345678-1234-4234-9234-123456789abc")
        );
        assert_eq!(
            q.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(
            q.get("agent_name_hint").map(String::as_str),
            Some("Ambient")
        );
        assert!(op.prepare_with(&metadata).is_err());
        Ok(())
    }
    #[test]
    fn callback_requires_state_and_issued_id_before_any_exchange()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut op = AuthOperation::new(
            crate::shape_tests::initial(),
            Some("http://127.0.0.1:12345/auth/callback".into()),
            false,
            Arc::new(AtomicBool::new(false)),
        )?;
        op.prepare_with(&metadata)?;
        assert!(
            op.accept_callback(
                "http://127.0.0.1:12345/auth/callback?state=foreign&code=code&client_id=issued"
            )
            .is_err()
        );
        let state = op.state.as_ref().ok_or("state")?.secret().clone();
        let url = format!(
            "http://127.0.0.1:12345/auth/callback?state={state}&code=fixture-code&client_id=issued_1"
        );
        assert_eq!(op.accept_callback(&url)?, "issued_1");
        assert!(op.accept_callback(&url).is_err());
        assert!(matches!(
            op.dispatch_with("different", &metadata),
            Err(AuthError::InvalidState)
        ));
        Ok(())
    }
    #[test]
    fn cancellation_during_discovery_is_reported_after_actual_call_returns() -> Result<(), AuthError>
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut op = AuthOperation::new(
            crate::shape_tests::initial(),
            Some("http://127.0.0.1:12345/auth/callback".into()),
            false,
            cancelled.clone(),
        )?;
        let result = op.prepare_with(&|_| {
            cancelled.store(true, Ordering::Release);
            Err(AuthError::TransportUnknown)
        });
        assert!(matches!(result, Err(AuthError::Cancelled)));
        Ok(())
    }
    #[test]
    fn callback_denial_duplicates_expiry_and_late_cancel_cannot_dispatch_or_commit()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::AtomicUsize;
        let mut op = AuthOperation::new(
            crate::shape_tests::initial(),
            Some("http://127.0.0.1:12345/auth/callback".into()),
            false,
            Arc::new(AtomicBool::new(false)),
        )?;
        op.prepare_with(&metadata)?;
        let state = op.state.as_ref().ok_or("state")?.secret().clone();
        for query in [
            format!("state={state}&state={state}&code=c&client_id=issued_1"),
            format!("state={state}&code=c&client_id=dynamic_agent_client"),
            format!("state={state}&code=c"),
        ] {
            assert!(
                op.accept_callback(&format!("http://127.0.0.1:12345/auth/callback?{query}"))
                    .is_err()
            );
        }
        assert!(matches!(
            op.accept_callback(&format!(
                "http://127.0.0.1:12345/auth/callback?state={state}&error=access_denied"
            )),
            Err(AuthError::Denied)
        ));
        let calls = AtomicUsize::new(0);
        assert!(
            op.dispatch_with("issued_1", &|r| {
                calls.fetch_add(1, Ordering::Relaxed);
                metadata(r)
            })
            .is_err()
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        let mut expired = prepared(crate::shape_tests::initial())?;
        expired.deadline = Instant::now() - Duration::from_secs(1);
        assert!(matches!(
            expired.dispatch_with("issued_1", &|r| {
                calls.fetch_add(1, Ordering::Relaxed);
                metadata(r)
            }),
            Err(AuthError::Expired)
        ));
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        let mut late = prepared(crate::shape_tests::initial())?;
        let response = token(signed(late.nonce.as_ref().ok_or("nonce")?.secret())?);
        let cancelled = late.cancelled.clone();
        assert!(matches!(
            late.dispatch_with("issued_1", &|_| {
                cancelled.store(true, Ordering::Release);
                reply(response.clone())
            }),
            Err(AuthError::Cancelled)
        ));
        Ok(())
    }
    fn key() -> Result<CoreRsaPrivateSigningKey, AuthError> {
        CoreRsaPrivateSigningKey::from_pem(
            include_str!("../tests/fixtures/idp-rsa.pem"),
            Some(JsonWebKeyId::new("fixture".into())),
        )
        .map_err(|_| AuthError::Internal)
    }
    fn signed(nonce: &str) -> Result<String, AuthError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| AuthError::Internal)?
            .as_secs();
        let claims: CoreIdTokenClaims = serde_json::from_value(serde_json::json!({"iss":endpoints::ISSUER,"sub":"fixture-subject","aud":"issued_1","exp":now+600,"iat":now,"nonce":nonce})).map_err(|_| AuthError::Internal)?;
        CoreIdToken::new(
            claims,
            &key()?,
            CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
            Some(&AccessToken::new("fixture-access".into())),
            None,
        )
        .map(|t| t.to_string())
        .map_err(|_| AuthError::Internal)
    }
    fn signed_metadata(request: HttpRequest) -> Result<HttpResponse, AuthError> {
        if request.uri() == endpoints::JWKS {
            return openidconnect::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(
                    serde_json::json!({"keys":[key()?.as_verification_key()]})
                        .to_string()
                        .into_bytes(),
                )
                .map_err(|_| AuthError::Internal);
        }
        metadata(request)
    }
    #[test]
    fn verified_exchange_binds_identity_and_is_dispatched_only_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut op = AuthOperation::new(
            crate::shape_tests::initial(),
            Some("http://127.0.0.1:12345/auth/callback".into()),
            false,
            Arc::new(AtomicBool::new(false)),
        )?;
        op.prepare_with(&signed_metadata)?;
        let nonce = op.nonce.as_ref().ok_or("nonce")?.secret().clone();
        let state = op.state.as_ref().ok_or("state")?.secret();
        op.accept_callback(&format!("http://127.0.0.1:12345/auth/callback?state={state}&code=fixture-code&client_id=issued_1"))?;
        let http = |r: HttpRequest| -> Result<HttpResponse, AuthError> {
            assert_eq!(r.method(), "POST");
            assert_eq!(r.uri().to_string(), endpoints::TOKEN);
            let q: BTreeMap<_, _> = Url::parse(&format!(
                "http://fixture/?{}",
                String::from_utf8_lossy(r.body())
            ))
            .map_err(|_| AuthError::Internal)?
            .query_pairs()
            .into_owned()
            .collect();
            assert_eq!(q.get("client_id").map(String::as_str), Some("issued_1"));
            assert_eq!(
                q.get("resource").map(String::as_str),
                Some("https://api.openai.com/v1")
            );
            assert!(q.contains_key("code_verifier"));
            assert!(!q.contains_key("client_secret"));
            openidconnect::http::Response::builder().status(200).header("content-type","application/json").body(serde_json::json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","token_type":"Bearer","expires_in":600,"scope":"openid resource.invoke chatgpt.tokens.use.direct","id_token":signed(&nonce)?}).to_string().into_bytes()).map_err(|_| AuthError::Internal)
        };
        let result = op.dispatch_with("issued_1", &http)?;
        assert_eq!(
            result
                .registration
                .identity
                .as_ref()
                .map(|i| i.subject.as_str()),
            Some("fixture-subject")
        );
        assert!(result.credentials.is_some());
        assert!(matches!(
            op.dispatch_with("issued_1", &http),
            Err(AuthError::InvalidState)
        ));
        Ok(())
    }
    fn prepared(snapshot: Snapshot) -> Result<AuthOperation, AuthError> {
        let mut op = AuthOperation::new(
            snapshot,
            Some("http://127.0.0.1:12345/auth/callback".into()),
            false,
            Arc::new(AtomicBool::new(false)),
        )?;
        op.prepare_with(&signed_metadata)?;
        let state = op.state.as_ref().ok_or(AuthError::Internal)?.secret();
        op.accept_callback(&format!("http://127.0.0.1:12345/auth/callback?state={state}&code=fixture-code&client_id=issued_1"))?;
        Ok(op)
    }
    fn reply(value: serde_json::Value) -> Result<HttpResponse, AuthError> {
        openidconnect::http::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(value.to_string().into_bytes())
            .map_err(|_| AuthError::Internal)
    }
    fn token(id: String) -> serde_json::Value {
        serde_json::json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","token_type":"Bearer","expires_in":600,"scope":"openid resource.invoke chatgpt.tokens.use.direct","id_token":id})
    }
    fn claims(nonce: &str) -> serde_json::Value {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        serde_json::json!({"iss":endpoints::ISSUER,"sub":"fixture-subject","aud":"issued_1","exp":now+600,"iat":now,"nonce":nonce})
    }
    fn sign(value: serde_json::Value, alg: CoreJwsSigningAlgorithm) -> Result<String, AuthError> {
        let claims: CoreIdTokenClaims =
            serde_json::from_value(value).map_err(|_| AuthError::Internal)?;
        CoreIdToken::new(
            claims,
            &key()?,
            alg,
            Some(&AccessToken::new("fixture-access".into())),
            None,
        )
        .map(|t| t.to_string())
        .map_err(|_| AuthError::Internal)
    }
    #[test]
    fn signed_claims_and_grants_are_validated_instead_of_requested_scopes()
    -> Result<(), Box<dyn std::error::Error>> {
        for case in 0..13 {
            let mut op = prepared(crate::shape_tests::initial())?;
            let nonce = op.nonce.as_ref().ok_or("nonce")?.secret();
            let mut value = claims(nonce);
            match case {
                0 => value["iss"] = "https://other.invalid".into(),
                1 => value["aud"] = "another_client".into(),
                2 => value["azp"] = "another_client".into(),
                3 => value["aud"] = serde_json::json!(["issued_1", "other"]),
                4 => value["nonce"] = "wrong".into(),
                5 => {
                    value.as_object_mut().ok_or("claims")?.remove("nonce");
                }
                6 => value["exp"] = 1.into(),
                7 => value["sub"] = "".into(),
                8 => value["sub"] = "x".repeat(1025).into(),
                _ => {}
            }
            let mut id = sign(
                value,
                if case == 9 {
                    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha512
                } else {
                    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256
                },
            )?;
            if case == 10 {
                id.pop();
                id.push('!');
            }
            let mut response = token(id);
            if case == 11 {
                response["access_token"] = "different".into();
            }
            if case == 12 {
                response["token_type"] = "Other".into();
            }
            let result = op.dispatch_with("issued_1", &|_| reply(response.clone()));
            assert!(result.is_err(), "case {case}");
        }
        for missing in ["scope", "refresh_token"] {
            let mut op = prepared(crate::shape_tests::initial())?;
            let mut response = token(signed(op.nonce.as_ref().ok_or("nonce")?.secret())?);
            response.as_object_mut().ok_or("response")?.remove(missing);
            let result = op.dispatch_with("issued_1", &|_| reply(response.clone()))?;
            assert!(result.credentials.is_none());
            assert_eq!(result.registration.state, 3);
            assert!(result.registration.hint.is_some());
        }
        Ok(())
    }
    #[test]
    fn refresh_keeps_identity_hint_and_scopes_without_an_id_token_and_never_repeats_unknown()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut initial = prepared(crate::shape_tests::initial())?;
        let response = token(signed(initial.nonce.as_ref().ok_or("nonce")?.secret())?);
        let saved = initial.dispatch_with("issued_1", &|_| reply(response.clone()))?;
        let mut returning = AuthOperation::new(
            saved.clone(),
            Some("http://127.0.0.1:12345/auth/callback".into()),
            false,
            Arc::new(AtomicBool::new(false)),
        )?;
        let returning_url = Url::parse(&returning.prepare_with(&signed_metadata)?.ok_or("url")?)?;
        let q: BTreeMap<_, _> = returning_url.query_pairs().into_owned().collect();
        assert_eq!(q.get("client_id").map(String::as_str), Some("issued_1"));
        assert_eq!(
            q.get("ext_agent_host_id").map(String::as_str),
            Some("urn:uuid:12345678-1234-4234-9234-123456789abc")
        );
        assert!(!q.contains_key("agent_name_hint"));
        assert!(q.contains_key("id_token_hint"));
        let state = returning.state.as_ref().ok_or("state")?.secret();
        assert_eq!(
            returning.accept_callback(&format!(
                "http://127.0.0.1:12345/auth/callback?state={state}&code=fixture"
            ))?,
            "issued_1"
        );
        for case in 0..5 {
            let mut op =
                AuthOperation::new(saved.clone(), None, true, Arc::new(AtomicBool::new(false)))?;
            op.prepare_with(&signed_metadata)?;
            let mut response = token(String::new());
            response
                .as_object_mut()
                .ok_or("response")?
                .remove("id_token");
            response.as_object_mut().ok_or("response")?.remove("scope");
            response["refresh_token"] = "replacement".into();
            if case == 1 {
                response
                    .as_object_mut()
                    .ok_or("response")?
                    .remove("refresh_token");
            }
            if (2..=3).contains(&case) {
                let mut value = claims(&saved.registration.hint.as_ref().ok_or("hint")?.nonce);
                value.as_object_mut().ok_or("claims")?.remove("nonce");
                if case == 3 {
                    value["sub"] = "other-subject".into();
                }
                response["id_token"] =
                    sign(value, CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256)?.into();
            }
            let result = op.dispatch_with("issued_1", &|r: HttpRequest| {
                let q: BTreeMap<_, _> = Url::parse(&format!(
                    "http://fixture/?{}",
                    String::from_utf8_lossy(r.body())
                ))
                .map_err(|_| AuthError::Internal)?
                .query_pairs()
                .into_owned()
                .collect();
                assert_eq!(
                    q.get("grant_type").map(String::as_str),
                    Some("refresh_token")
                );
                assert!(!q.contains_key("scope"));
                assert!(!q.contains_key("code_verifier"));
                if case == 4 {
                    Err(AuthError::TransportUnknown)
                } else {
                    reply(response.clone())
                }
            });
            if [1, 3, 4].contains(&case) {
                assert!(result.is_err());
            } else {
                let result = result?;
                assert_eq!(
                    result.credentials.as_ref().map(|c| c.refresh.as_str()),
                    Some("replacement")
                );
                assert_eq!(
                    result
                        .registration
                        .identity
                        .as_ref()
                        .map(|i| i.subject.as_str()),
                    Some("fixture-subject")
                );
                if case == 0 {
                    assert_eq!(
                        result.registration.hint.as_ref().map(|h| h.token.as_str()),
                        saved.registration.hint.as_ref().map(|h| h.token.as_str())
                    );
                }
            }
            assert!(matches!(
                op.dispatch_with("issued_1", &signed_metadata),
                Err(AuthError::InvalidState)
            ));
        }
        Ok(())
    }
    #[test]
    fn signing_idp_runs_discovery_jwks_and_one_exchange_through_real_local_http()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::{
            io::{BufRead, BufReader, Read, Write},
            net::TcpListener,
            sync::Mutex,
            thread,
        };
        let server = TcpListener::bind("127.0.0.1:0")?;
        let origin = format!("http://{}", server.local_addr()?);
        let nonce = Arc::new(Mutex::new(String::new()));
        let server_nonce = nonce.clone();
        let worker = thread::spawn(move || -> Result<Vec<String>, AuthError> {
            let mut paths = Vec::new();
            for _ in 0..3 {
                let (mut stream, _) = server.accept().map_err(|_| AuthError::Internal)?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .map_err(|_| AuthError::Internal)?;
                let mut input =
                    BufReader::new(stream.try_clone().map_err(|_| AuthError::Internal)?);
                let mut first = String::new();
                input
                    .read_line(&mut first)
                    .map_err(|_| AuthError::Internal)?;
                let path = first
                    .split_whitespace()
                    .nth(1)
                    .ok_or(AuthError::Internal)?
                    .to_owned();
                paths.push(path.clone());
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    input
                        .read_line(&mut line)
                        .map_err(|_| AuthError::Internal)?;
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().map_err(|_| AuthError::Internal)?;
                    }
                }
                if length > 65536 {
                    return Err(AuthError::ResourceLimit);
                }
                let mut request_body = vec![0; length];
                input
                    .read_exact(&mut request_body)
                    .map_err(|_| AuthError::Internal)?;
                let body = match path.as_str() {
                    "/discovery" => metadata(
                        openidconnect::http::Request::builder()
                            .uri(endpoints::DISCOVERY)
                            .body(vec![])
                            .map_err(|_| AuthError::Internal)?,
                    )?
                    .into_body(),
                    "/jwks" => serde_json::json!({"keys":[key()?.as_verification_key()]})
                        .to_string()
                        .into_bytes(),
                    "/token" => {
                        let q: BTreeMap<_, _> = Url::parse(&format!(
                            "http://fixture/?{}",
                            String::from_utf8_lossy(&request_body)
                        ))
                        .map_err(|_| AuthError::Internal)?
                        .query_pairs()
                        .into_owned()
                        .collect();
                        if q.get("client_id").map(String::as_str) != Some("issued_1")
                            || !q.contains_key("code_verifier")
                        {
                            return Err(AuthError::InvalidInput);
                        }
                        token(signed(
                            &server_nonce.lock().map_err(|_| AuthError::Internal)?,
                        )?)
                        .to_string()
                        .into_bytes()
                    }
                    _ => return Err(AuthError::InvalidInput),
                };
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).map_err(|_|AuthError::Internal)?;
                stream.write_all(&body).map_err(|_| AuthError::Internal)?;
            }
            Ok(paths)
        });
        let routes = [
            (endpoints::DISCOVERY, "/discovery", "GET"),
            (endpoints::JWKS, "/jwks", "GET"),
            (endpoints::TOKEN, "/token", "POST"),
        ];
        let transport = transport::Transport::test_routes(
            routes
                .iter()
                .map(|(_, path, method)| ((*method).into(), format!("{origin}{path}")))
                .collect(),
        )?;
        let http = |request: HttpRequest| -> Result<HttpResponse, AuthError> {
            let (mut parts, body) = request.into_parts();
            let path = routes
                .iter()
                .find(|(url, _, _)| parts.uri == *url)
                .ok_or(AuthError::InvalidInput)?
                .1;
            parts.uri = format!("{origin}{path}")
                .parse()
                .map_err(|_| AuthError::InvalidInput)?;
            transport.call(HttpRequest::from_parts(parts, body))
        };
        let mut op = AuthOperation::new(
            crate::shape_tests::initial(),
            Some("http://127.0.0.1:12345/auth/callback".into()),
            false,
            Arc::new(AtomicBool::new(false)),
        )?;
        op.prepare_with(&http)?;
        *nonce.lock().map_err(|_| "nonce")? = op.nonce.as_ref().ok_or("nonce")?.secret().clone();
        let state = op.state.as_ref().ok_or("state")?.secret();
        op.accept_callback(&format!(
            "http://127.0.0.1:12345/auth/callback?state={state}&code=fixture&client_id=issued_1"
        ))?;
        assert!(op.dispatch_with("issued_1", &http)?.credentials.is_some());
        assert_eq!(
            worker.join().map_err(|_| "worker")??,
            vec!["/discovery", "/jwks", "/token"]
        );
        Ok(())
    }
}

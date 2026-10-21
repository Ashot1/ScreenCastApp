//! Built-in OAuth registry of managed servers (ADR-021 "Sign-in", ADR-022).
//!
//! Keyed by `ManagedServerId`, not by host: the registry is fixed code, never user input.
//! The Desktop client is injected at build time by `build.rs`.

// Defines `pub(crate) const GOOGLE_CLIENT: Option<(&str, &str)>`.
include!(concat!(env!("OUT_DIR"), "/google_client.rs"));

pub(crate) struct OAuthRegistry {
    /// Exact `iss` of the authorization response; no trailing slash.
    pub issuer: &'static str,
    pub authorize: &'static str,
    pub token: &'static str,
    /// RFC 7009 revocation: sign-out and removal revoke the refresh token at the provider.
    pub revoke: &'static str,
    pub scopes: &'static [&'static str],
    /// RFC 8707 resource; `None` when the provider ignores it (Google).
    pub resource: Option<&'static str>,
    /// Extra authorization request parameters.
    pub extra: &'static [(&'static str, &'static str)],
    /// `(client_id, client_secret)` of the Desktop ("installed") client. By Google's definition
    /// the secret of an installed app is not confidential, but it is never logged or stored.
    /// `None` in builds without the client.
    pub client: Option<(&'static str, &'static str)>,
}

const GOOGLE: OAuthRegistry = OAuthRegistry {
    issuer: "https://accounts.google.com",
    authorize: "https://accounts.google.com/o/oauth2/v2/auth",
    token: "https://oauth2.googleapis.com/token",
    revoke: "https://oauth2.googleapis.com/revoke",
    // `userinfo.email` is required: the server rejects a token without it.
    scopes: &[
        "openid",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/drive.readonly",
        "https://www.googleapis.com/auth/documents.readonly",
    ],
    resource: None,
    extra: &[("access_type", "offline"), ("prompt", "consent")],
    client: GOOGLE_CLIENT,
};

pub(crate) fn registry(id: ambient_core::ManagedServerId) -> &'static OAuthRegistry {
    match id {
        ambient_core::ManagedServerId::GoogleWorkspace => &GOOGLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_endpoints_are_https_on_expected_hosts() -> Result<(), Box<dyn std::error::Error>> {
        let google = registry(ambient_core::ManagedServerId::GoogleWorkspace);
        assert!(!google.issuer.ends_with('/'));
        for (url, host) in [
            (google.issuer, "accounts.google.com"),
            (google.authorize, "accounts.google.com"),
            (google.token, "oauth2.googleapis.com"),
            (google.revoke, "oauth2.googleapis.com"),
        ] {
            let parsed = reqwest::Url::parse(url)?;
            assert_eq!(parsed.scheme(), "https");
            assert_eq!(parsed.host_str(), Some(host));
        }
        Ok(())
    }

    #[test]
    fn google_scopes_keep_required_email_scope_and_order() {
        let google = registry(ambient_core::ManagedServerId::GoogleWorkspace);
        assert_eq!(google.scopes[0], "openid");
        assert!(
            google
                .scopes
                .contains(&"https://www.googleapis.com/auth/userinfo.email")
        );
        assert!(google.resource.is_none());
        assert!(google.extra.contains(&("access_type", "offline")));
    }
}

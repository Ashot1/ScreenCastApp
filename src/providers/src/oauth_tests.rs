//! Google sign-in against a loopback authorization-server fixture (ADR-021 «Testing
//! Decisions» 24B): PKCE, state, the exact issuer, the client secret in the body, no resource,
//! refresh with and without rotation and `invalid_grant`.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const CLIENT: (&str, &str) = ("client-1.apps.googleusercontent.com", "test-secret-1");
const REDIRECT: &str = "http://127.0.0.1:53682/";

/// Answers one POST with `body` and status; returns the request (headers and form) it got.
fn token_server(status: &str, body: &str) -> Result<(&'static str, thread::JoinHandle<String>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url: &'static str = format!("http://{}/token", listener.local_addr()?).leak();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let worker = thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return String::new();
        };
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        // Headers, then the form body by Content-Length.
        while let Ok(n) = stream.read(&mut chunk) {
            request.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&request).into_owned();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .and_then(|n| n.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8_lossy(&request).into_owned()
    });
    Ok((url, worker))
}
fn google(token: &'static str) -> OAuthRegistry {
    let r = registry(ManagedServerId::GoogleWorkspace);
    OAuthRegistry {
        issuer: r.issuer,
        authorize: r.authorize,
        token,
        revoke: token,
        scopes: r.scopes,
        resource: r.resource,
        extra: r.extra,
        client: Some(CLIENT),
    }
}
fn form(request: &str) -> std::collections::BTreeMap<String, String> {
    let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
    reqwest::Url::parse(&format!("http://x/?{body}"))
        .map(|u| u.query_pairs().into_owned().collect())
        .unwrap_or_default()
}
fn json(o: &OAuthOutcome) -> serde_json::Value {
    serde_json::to_value(o).unwrap_or_default()
}

#[test]
fn the_authorization_url_carries_pkce_state_scopes_and_offline_consent_without_resource() -> Result
{
    let r = google("https://oauth2.googleapis.com/token");
    let start = begin_with(&r, CLIENT, REDIRECT).ok_or("begin")?;
    let url = reqwest::Url::parse(&start.url)?;
    assert_eq!(
        (url.scheme(), url.host_str(), url.path()),
        ("https", Some("accounts.google.com"), "/o/oauth2/v2/auth")
    );
    let q: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(q["client_id"], CLIENT.0);
    assert_eq!(q["redirect_uri"], REDIRECT);
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["state"], start.state);
    assert_eq!(q["access_type"], "offline");
    assert_eq!(q["prompt"], "consent");
    assert_eq!(
        q["scope"],
        "openid https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/drive.readonly https://www.googleapis.com/auth/documents.readonly"
    );
    // The secret never goes to the browser; Google ignores RFC 8707, so none is sent.
    assert!(!q.contains_key("client_secret") && !q.contains_key("resource"));
    let challenge = PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(
        start.verifier.clone(),
    ));
    assert_eq!(q["code_challenge"], *challenge.as_str());
    // Only the exact loopback redirect native listens on.
    for bad in [
        "http://localhost:53682/",
        "https://127.0.0.1:53682/",
        "http://127.0.0.1/",
        "http://127.0.0.1:53682/auth/callback",
        "http://127.0.0.1:53682/?x=1",
    ] {
        assert!(begin_with(&r, CLIENT, bad).is_none(), "{bad}");
    }
    Ok(())
}

#[test]
fn the_callback_needs_this_state_and_the_exact_issuer_and_the_exchange_sends_the_secret() -> Result
{
    let ok = r#"{"access_token":"ya29.access-1","expires_in":3599,"refresh_token":"1//refresh-1","refresh_token_expires_in":604799,"scope":"openid","token_type":"Bearer"}"#;
    let good = "state=s-1&code=c-1&scope=openid&iss=https%3A%2F%2Faccounts.google.com";
    // Rejected before any network use: wrong or repeated state, trailing-slash or missing iss,
    // no code. The token server is never reached.
    let r = google("http://127.0.0.1:9/token");
    for (query, expected) in [
        (
            "state=other&code=c-1&iss=https%3A%2F%2Faccounts.google.com",
            "invalid",
        ),
        (
            "state=s-1&state=s-1&code=c-1&iss=https%3A%2F%2Faccounts.google.com",
            "invalid",
        ),
        (
            "state=s-1&code=c-1&iss=https%3A%2F%2Faccounts.google.com%2F",
            "invalid",
        ),
        ("state=s-1&code=c-1", "invalid"),
        ("state=s-1&iss=https%3A%2F%2Faccounts.google.com", "invalid"),
        ("state=s-1&error=access_denied", "denied"),
        ("state=s-1&error=server_error", "invalid"),
    ] {
        let o = exchange_with(&r, CLIENT, REDIRECT, query, "s-1", "verifier-1");
        assert_eq!(json(&o)["status"], expected, "{query}");
    }
    let (url, server) = token_server("200 OK", ok)?;
    let o = exchange_with(&google(url), CLIENT, REDIRECT, good, "s-1", "verifier-1");
    assert_eq!(
        json(&o),
        serde_json::json!({"status":"ok","access_token":"ya29.access-1","refresh_token":"1//refresh-1","expires_in":3599})
    );
    let request = server.join().map_err(|_| "server")?;
    let f = form(&request);
    assert_eq!(f["grant_type"], "authorization_code");
    assert_eq!(f["code"], "c-1");
    assert_eq!(f["code_verifier"], "verifier-1");
    assert_eq!(f["redirect_uri"], REDIRECT);
    assert_eq!(f["client_id"], CLIENT.0);
    // Google refuses the exchange without the secret (live check); it goes in the body.
    assert_eq!(f["client_secret"], CLIENT.1);
    assert!(!f.contains_key("resource"));
    assert!(!request.to_ascii_lowercase().contains("authorization:"));
    // An exchange without a refresh token cannot keep the sign-in: it is a failure.
    let (url, server) = token_server(
        "200 OK",
        r#"{"access_token":"ya29.a","expires_in":3599,"token_type":"Bearer"}"#,
    )?;
    let o = exchange_with(&google(url), CLIENT, REDIRECT, good, "s-1", "verifier-1");
    assert_eq!(json(&o)["status"], "failed");
    server.join().map_err(|_| "server")?;
    let (url, server) = token_server("400 Bad Request", r#"{"error":"invalid_grant"}"#)?;
    let o = exchange_with(&google(url), CLIENT, REDIRECT, good, "s-1", "verifier-1");
    assert_eq!(json(&o)["status"], "invalid");
    server.join().map_err(|_| "server")?;
    Ok(())
}

#[test]
fn refresh_keeps_the_token_without_rotation_returns_a_rotated_one_and_names_invalid_grant() -> Result
{
    // Google does not rotate: no new refresh token in the answer.
    let (url, server) = token_server(
        "200 OK",
        r#"{"access_token":"ya29.access-2","expires_in":3599,"refresh_token_expires_in":500000,"scope":"openid","token_type":"Bearer"}"#,
    )?;
    let o = refresh_with(&google(url), CLIENT, "1//refresh-1");
    assert_eq!(
        json(&o),
        serde_json::json!({"status":"ok","access_token":"ya29.access-2","expires_in":3599})
    );
    let f = form(&server.join().map_err(|_| "server")?);
    assert_eq!(f["grant_type"], "refresh_token");
    assert_eq!(f["refresh_token"], "1//refresh-1");
    assert_eq!(f["client_secret"], CLIENT.1);
    // A rotating server: the new refresh token reaches native.
    let (url, server) = token_server(
        "200 OK",
        r#"{"access_token":"ya29.access-3","expires_in":3599,"refresh_token":"1//refresh-2","token_type":"Bearer"}"#,
    )?;
    let o = refresh_with(&google(url), CLIENT, "1//refresh-1");
    assert_eq!(json(&o)["refresh_token"], "1//refresh-2");
    server.join().map_err(|_| "server")?;
    // An expired testing grant: sign in again. Other errors change nothing.
    let (url, server) = token_server(
        "400 Bad Request",
        r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
    )?;
    assert_eq!(
        json(&refresh_with(&google(url), CLIENT, "1//r"))["status"],
        "invalid_grant"
    );
    server.join().map_err(|_| "server")?;
    let (url, server) = token_server("500 Internal Server Error", r#"{"error":"internal"}"#)?;
    assert_eq!(
        json(&refresh_with(&google(url), CLIENT, "1//r"))["status"],
        "failed"
    );
    server.join().map_err(|_| "server")?;
    // Nothing listening: a network failure, not a lost grant.
    assert_eq!(
        json(&refresh_with(
            &google("http://127.0.0.1:9/token"),
            CLIENT,
            "1//r"
        ))["status"],
        "failed"
    );
    Ok(())
}

#[test]
fn sign_out_revokes_the_refresh_token_once_and_a_failure_is_only_reported() -> Result {
    let (url, server) = token_server("200 OK", "")?;
    let o = revoke_with(&google(url), CLIENT, "1//refresh-1");
    assert_eq!(
        serde_json::to_value(&o)?,
        serde_json::json!({"status":"revoked"})
    );
    let request = server.join().map_err(|_| "server")?;
    let f = form(&request);
    assert_eq!(f["token"], "1//refresh-1");
    assert_eq!(f["token_type_hint"], "refresh_token");
    assert_eq!(f["client_id"], CLIENT.0);
    // Already revoked or refused: reported, never retried.
    let (url, server) = token_server("400 Bad Request", r#"{"error":"invalid_token"}"#)?;
    assert_eq!(
        serde_json::to_value(revoke_with(&google(url), CLIENT, "1//r"))?["status"],
        "failed"
    );
    server.join().map_err(|_| "server")?;
    // Nothing listening: a network failure.
    assert_eq!(
        serde_json::to_value(revoke_with(
            &google("http://127.0.0.1:9/revoke"),
            CLIENT,
            "1//r"
        ))?["status"],
        "failed"
    );
    Ok(())
}

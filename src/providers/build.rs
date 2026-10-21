//! Build-time injection of the built-in Google Desktop OAuth client (ADR-021 «Вход», D63). The
//! client JSON stays outside the repository; `AMBIENT_GOOGLE_OAUTH_CLIENT` names it (the macOS
//! app build sets it). Without it the build has no Google client and sign-in reports that.
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=AMBIENT_GOOGLE_OAUTH_CLIENT");
    let client = env::var_os("AMBIENT_GOOGLE_OAUTH_CLIENT").and_then(|path| {
        let path = PathBuf::from(path);
        println!("cargo:rerun-if-changed={}", path.display());
        // A missing file is a build without the client; a present but unusable one is reported.
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(_) => {
                println!("cargo:warning=the Google OAuth client file cannot be read");
                return None;
            }
        };
        let parsed = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
        if parsed.is_none() {
            println!("cargo:warning=the Google OAuth client file is not valid JSON");
        }
        let json = parsed?;
        let Some(installed) = json.get("installed") else {
            println!(
                "cargo:warning=the Google OAuth client file is not a Desktop (installed) client"
            );
            return None;
        };
        let field = |name: &str| {
            installed
                .get(name)
                .and_then(|v| v.as_str())
                .filter(|v| {
                    (1..=256).contains(&v.len()) && v.bytes().all(|b| (33..=126).contains(&b))
                })
                .map(str::to_owned)
        };
        let client = field("client_id").zip(field("client_secret"));
        if client.is_none() {
            println!("cargo:warning=the Google OAuth client file has no usable installed client");
        }
        client
    });
    let code = match client {
        Some((id, secret)) => format!("Some(({id:?}, {secret:?}))"),
        None => "None".to_owned(),
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default()).join("google_client.rs");
    if fs::write(
        &out,
        format!("pub(crate) const GOOGLE_CLIENT: Option<(&str, &str)> = {code};\n"),
    )
    .is_err()
    {
        println!("cargo:warning=cannot write the Google client constant");
        std::process::exit(1);
    }
}

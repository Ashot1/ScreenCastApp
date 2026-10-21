//! Fixed ChatGPT sign-in endpoints and public-client registration values.

pub const ISSUER: &str = "https://auth.openai.com";
pub const DISCOVERY: &str = "https://auth.openai.com/.well-known/openid-configuration";
pub const JWKS: &str = "https://auth.openai.com/.well-known/jwks.json";
pub const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
pub const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
pub const RESOURCE: &str = "https://api.openai.com/v1";

pub const DYNAMIC_CLIENT_ID: &str = "dynamic_agent_client";
pub const AGENT_NAME: &str = "Ambient";
pub const SCOPES: [&str; 6] = [
    "openid",
    "profile",
    "email",
    "offline_access",
    "resource.invoke",
    "chatgpt.tokens.use.direct",
];

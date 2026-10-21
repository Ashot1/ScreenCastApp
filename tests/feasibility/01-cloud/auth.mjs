import * as oidc from 'openid-client';

const issuer = 'https://auth.openai.com';
const authorize = `${issuer}/api/accounts/authorize`;
const tokenEndpoint = `${issuer}/api/accounts/oauth/token`;
const jwksUri = `${issuer}/.well-known/jwks.json`;
const scope = 'openid profile email offline_access resource.invoke chatgpt.tokens.use.direct';
const resource = 'https://api.openai.com/v1';
const reject = () => { throw new Error('invalid_sign_in'); };
const requiredScopes = ['openid', 'resource.invoke', 'chatgpt.tokens.use.direct'];
const maximumTokenLength = 16_384;

// Library-owned parsing and signature checks use bounded, pinned HTTPS fetches without redirects or retries.
async function boundedResponse(response, maximumBytes = 512 * 1024) {
  if (!response.body) reject();
  const reader = response.body.getReader();
  let length = 0;
  const chunks = [];
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      length += value.byteLength;
      if (length > maximumBytes) reject();
      chunks.push(value);
    }
  } finally { await reader.cancel(); reader.releaseLock(); }
  return new Response(Buffer.concat(chunks), { status: response.status, headers: response.headers });
}

export async function prepareSignIn({ hostId, redirectUri, fetcher = fetch, signal, registration }) {
  const redirect = new URL(redirectUri);
  if (redirect.protocol !== 'http:' || redirect.hostname !== '127.0.0.1' || !redirect.port ||
      redirect.pathname !== '/auth/callback' || redirect.search || redirect.hash || redirect.username || redirect.password ||
      !/^urn:uuid:[0-9a-f-]{36}$/.test(hostId)) reject();
  if (registration !== undefined && !isRegistration(registration)) reject();
  const client = await discoverClient(registration?.client_id ?? 'dynamic_agent_client', fetcher, signal);
  const { discovered, metadata, securedFetch } = client;
  const verifier = oidc.randomPKCECodeVerifier();
  const state = oidc.randomState();
  const nonce = oidc.randomNonce();
  const authorizationUrl = oidc.buildAuthorizationUrl(discovered, {
    redirect_uri: redirectUri, response_type: 'code', scope, resource, state, nonce,
    code_challenge: await oidc.calculatePKCECodeChallenge(verifier), code_challenge_method: 'S256',
    ...(!registration ? { agent_name_hint: 'Ambient Assistant Feasibility' } : {}), ext_agent_host_id: hostId,
  });
  return { authorizationUrl, state, nonce, verifier, redirectUri, metadata, securedFetch, registration, used: false };
}

async function discoverClient(clientId, fetcher, signal) {
  const securedFetch = async (url, options = {}) => {
    const destination = String(url);
    const method = options.method ?? 'GET';
    if (!((destination === `${issuer}/.well-known/openid-configuration` || destination === jwksUri) && method === 'GET') &&
        !(destination === tokenEndpoint && method === 'POST')) reject();
    const signals = [signal, options.signal, AbortSignal.timeout(15_000)].filter(Boolean);
    return boundedResponse(await fetcher(url, { ...options, redirect: 'error', signal: AbortSignal.any(signals) }));
  };
  const discovered = await oidc.discovery(new URL(issuer), clientId, undefined, oidc.None(), {
    [oidc.customFetch]: securedFetch, timeout: 15,
  });
  const metadata = discovered.serverMetadata();
  if (metadata.issuer !== issuer || metadata.authorization_endpoint !== authorize || metadata.token_endpoint !== tokenEndpoint ||
      metadata.jwks_uri !== jwksUri || !metadata.id_token_signing_alg_values_supported?.includes('RS256')) reject();
  const configuration = new oidc.Configuration(metadata, clientId,
    { id_token_signed_response_alg: 'RS256', [oidc.clockTolerance]: 0 }, oidc.None());
  configuration[oidc.customFetch] = securedFetch;
  configuration.timeout = 15;
  oidc.enableNonRepudiationChecks(configuration);
  return { discovered, metadata, securedFetch, configuration };
}

export async function exchangeCallback(pending, callbackUrl) {
  if (pending.used) reject();
  pending.used = true;
  const url = new URL(callbackUrl);
  const redirect = new URL(pending.redirectUri);
  const single = key => url.searchParams.getAll(key).length === 1 && url.searchParams.get(key);
  if (url.origin !== redirect.origin || url.pathname !== redirect.pathname || url.hash || url.username || url.password ||
      single('state') !== pending.state || url.searchParams.has('error')) reject();
  if (url.searchParams.getAll('client_id').length > 1) reject();
  const returnedId = url.searchParams.has('client_id') ? single('client_id') : undefined;
  if (url.searchParams.has('client_id') && !returnedId) reject();
  if (pending.registration && returnedId && returnedId !== pending.registration.client_id) reject();
  const clientId = pending.registration?.client_id ?? returnedId;
  if (!single('code') || !clientId || clientId === 'dynamic_agent_client' || !/^[A-Za-z0-9_-]{1,512}$/.test(clientId)) reject();
  // The initial placeholder must never become the token audience or the exchanging client ID.
  const config = new oidc.Configuration(pending.metadata, clientId, { id_token_signed_response_alg: 'RS256', [oidc.clockTolerance]: 0 }, oidc.None());
  config[oidc.customFetch] = pending.securedFetch;
  config.timeout = 15;
  // Code flow checks claims/nonce; this opt-in also verifies the ID-token signature using the pinned JWKS.
  oidc.enableNonRepudiationChecks(config);
  const tokens = await oidc.authorizationCodeGrant(config, url, {
    pkceCodeVerifier: pending.verifier, expectedState: pending.state, expectedNonce: pending.nonce, idTokenExpected: true,
  }, { resource });
  const payload = tokens.claims();
  const granted = typeof tokens.scope === 'string' ? tokens.scope.split(/\s+/) : [];
  if (payload.nonce !== pending.nonce || typeof payload.sub !== 'string' || !payload.sub || payload.sub.length > 1024 ||
      (pending.registration && (payload.iss !== pending.registration.issuer || payload.sub !== pending.registration.subject)) ||
      tokens.token_type?.toLowerCase() !== 'bearer' || !Number.isSafeInteger(tokens.expires_in) || tokens.expires_in <= 0 ||
      !validToken(tokens.access_token)) reject();
  const registration = { client_id: clientId, issuer: payload.iss, subject: payload.sub };
  // Retain the verified grant only after the complete session has passed local checks.
  if (!requiredScopes.every(value => granted.includes(value))) {
    return { registration, blockedReason: 'subscription_scope_not_granted' };
  }
  if (!validToken(tokens.refresh_token)) return { registration, blockedReason: 'refresh_token_not_granted' };
  return { accessToken: tokens.access_token, refreshToken: tokens.refresh_token, expiresAt: Date.now() + tokens.expires_in * 1000,
    grantedScopes: granted, nonce: pending.nonce, registration };
}

function sameRegistration(a, b) {
  return isRegistration(a) && isRegistration(b) && a.client_id === b.client_id && a.issuer === b.issuer && a.subject === b.subject;
}

function isRegistration(value) {
  return Boolean(value && typeof value === 'object' && !Array.isArray(value) && value.issuer === issuer &&
    typeof value.client_id === 'string' && /^[A-Za-z0-9_-]{1,512}$/.test(value.client_id) &&
    value.client_id !== 'dynamic_agent_client' && typeof value.subject === 'string' &&
    value.subject.length > 0 && value.subject.length <= 1024);
}

function validToken(value) {
  return typeof value === 'string' && value.length > 0 && Buffer.byteLength(value) <= maximumTokenLength;
}

function requiredScopeSet(scopes) {
  return validScopeList(scopes) && requiredScopes.every(value => scopes.includes(value));
}

function validScopeList(scopes) {
  return Array.isArray(scopes) && scopes.length <= 128 && scopes.every(value =>
    typeof value === 'string' && /^[A-Za-z0-9:._/-]{1,128}$/.test(value));
}

function runtimeSession(value, registration) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || !sameRegistration(value.registration, registration) ||
      !validToken(value.access_token) || !validToken(value.refresh_token) ||
      !Number.isSafeInteger(value.expires_at_ms) || value.expires_at_ms <= 0 || !requiredScopeSet(value.granted_scopes) ||
      typeof value.nonce !== 'string' || !value.nonce || Buffer.byteLength(value.nonce) > 1024) return null;
  return { accessToken: value.access_token, refreshToken: value.refresh_token, expiresAt: value.expires_at_ms,
    grantedScopes: value.granted_scopes, nonce: value.nonce, registration: value.registration };
}

function storedSession(value) {
  return { access_token: value.accessToken, refresh_token: value.refreshToken, expires_at_ms: value.expiresAt,
    granted_scopes: value.grantedScopes, nonce: value.nonce, registration: value.registration };
}

export async function refreshSession(session, { fetcher = fetch, signal, now = Date.now } = {}) {
  if (!session || !isRegistration(session.registration) || !validToken(session.refreshToken) ||
      !requiredScopeSet(session.grantedScopes) || typeof session.nonce !== 'string' || !session.nonce ||
      Buffer.byteLength(session.nonce) > 1024) reject();
  const client = await discoverClient(session.registration.client_id, fetcher, signal);
  const tokens = await oidc.refreshTokenGrant(client.configuration, session.refreshToken, { resource });
  if (tokens.token_type?.toLowerCase() !== 'bearer' || !validToken(tokens.access_token) ||
      !Number.isSafeInteger(tokens.expires_in) || tokens.expires_in <= 0 || !validToken(tokens.refresh_token)) reject();
  const payload = tokens.id_token ? tokens.claims() : undefined;
  // The library validates signature/issuer/audience; only the prior identity and optional original nonce live here.
  if (payload && (!Number.isSafeInteger(payload.exp) || payload.exp <= Math.floor(now() / 1000) ||
      payload.sub !== session.registration.subject || (payload.nonce !== undefined && payload.nonce !== session.nonce))) reject();
  let grantedScopes = session.grantedScopes;
  if (tokens.scope !== undefined) {
    if (typeof tokens.scope !== 'string' || !tokens.scope.trim()) reject();
    const responseScopes = tokens.scope.split(/\s+/);
    if (!validScopeList(responseScopes)) reject();
    const returned = new Set(responseScopes);
    grantedScopes = session.grantedScopes.filter(value => returned.has(value));
  }
  const expiresAt = now() + tokens.expires_in * 1000;
  if (!Number.isSafeInteger(expiresAt) || !requiredScopeSet(grantedScopes)) reject();
  return { accessToken: tokens.access_token, refreshToken: tokens.refresh_token,
    expiresAt, grantedScopes, nonce: session.nonce, registration: session.registration };
}

export async function authenticateSession({ hostId, registration, store, signIn, fetcher = fetch, signal,
  refresh = (session, options) => refreshSession(session, options), now = Date.now }) {
  if (!store || typeof store.read !== 'function' || typeof store.add !== 'function' ||
      typeof store.delete !== 'function' || typeof signIn !== 'function') return { registration, blockedReason: 'session_storage_failed' };
  if (registration !== undefined && !isRegistration(registration)) return { blockedReason: 'session_binding_invalid' };
  if (registration) {
    let cached;
    try { cached = await store.read(hostId, registration); }
    catch { return { registration, blockedReason: 'session_storage_failed' }; }
    if (cached) {
      const session = runtimeSession(cached, registration);
      if (!session) return { registration, blockedReason: 'session_binding_invalid' };
      if (session.expiresAt > now() + 15_000) return session;
      try { await store.delete(hostId, registration); }
      catch { return { registration, blockedReason: 'session_storage_failed' }; }
      let rotated;
      try { rotated = await refresh(session, { fetcher, signal, now }); }
      catch { return { registration, blockedReason: 'session_refresh_failed_reauthentication_required' }; }
      if (!rotated || !runtimeSession(storedSession(rotated), registration) || rotated.expiresAt <= now()) {
        return { registration, blockedReason: 'session_refresh_failed_reauthentication_required' };
      }
      try { await store.add(hostId, registration, storedSession(rotated)); }
      catch { return { registration, blockedReason: 'session_storage_failed' }; }
      return rotated;
    }
  }
  let fresh;
  try { fresh = await signIn({ hostId, registration, signal, fetcher }); }
  catch { return { registration, blockedReason: 'sign_in_failed' }; }
  if (fresh?.blockedReason) return fresh;
  const binding = registration ?? fresh?.registration;
  if (!sameRegistration(fresh?.registration, binding)) return { registration: binding, blockedReason: 'session_binding_invalid' };
  if (!runtimeSession(storedSession(fresh), binding) || fresh.expiresAt <= now()) {
    return { registration: binding, blockedReason: 'subscription_scope_not_granted' };
  }
  try { await store.add(hostId, binding, storedSession(fresh)); }
  catch { return { registration: binding, blockedReason: 'session_storage_failed' }; }
  return fresh;
}

import { beforeAll, describe, expect, it } from 'vitest';
import { generateKeyPair, exportJWK, SignJWT } from 'jose';
import { prepareSignIn, exchangeCallback, authenticateSession, refreshSession } from './auth.mjs';

const issuer = 'https://auth.openai.com';
const tokenUrl = `${issuer}/api/accounts/oauth/token`;
const jwksUrl = `${issuer}/.well-known/jwks.json`;
const redirectUri = 'http://127.0.0.1:12345/auth/callback';
const registration = { client_id: 'oaiapp_fixture', issuer, subject: 'synthetic-subject' };
const scopes = 'openid profile email offline_access resource.invoke chatgpt.tokens.use.direct';
const metadata = { issuer, authorization_endpoint: `${issuer}/api/accounts/authorize`, token_endpoint: tokenUrl,
  jwks_uri: jwksUrl, id_token_signing_alg_values_supported: ['RS256'], response_types_supported: ['code'],
  subject_types_supported: ['public'], token_endpoint_auth_methods_supported: ['none'] };
let privateKey, publicJwk;
beforeAll(async () => {
  const keys = await generateKeyPair('RS256');
  privateKey = keys.privateKey;
  publicJwk = { ...await exportJWK(keys.publicKey), kid: 'fixture', alg: 'RS256', use: 'sig' };
});
async function fixture({ claims = {}, token = {}, mutateSignature = false, discovery = {}, savedRegistration } = {}) {
  const calls = [];
  let pending;
  const fetcher = async (url, options) => {
    calls.push({ url: String(url), options });
    if (String(url).endsWith('/.well-known/openid-configuration')) return Response.json({ ...metadata, ...discovery });
    if (String(url) === jwksUrl) return Response.json({ keys: [publicJwk] });
    if (String(url) === tokenUrl) {
      const now = Math.floor(Date.now() / 1000);
      let idToken = await new SignJWT({ nonce: pending.nonce, ...claims }).setProtectedHeader({ alg: 'RS256', kid: 'fixture' })
        .setIssuer(claims.iss ?? issuer).setAudience(claims.aud ?? 'oaiapp_fixture').setSubject(claims.sub ?? 'synthetic-subject')
        .setIssuedAt().setExpirationTime(claims.exp ?? now + 300).sign(privateKey);
      if (mutateSignature) { const pieces = idToken.split('.'); pieces[2] = (pieces[2][0] === 'A' ? 'B' : 'A') + pieces[2].slice(1); idToken = pieces.join('.'); }
      return Response.json({ token_type: 'Bearer', access_token: 'SYNTHETIC_ACCESS', refresh_token: 'SYNTHETIC_REFRESH',
        id_token: idToken, expires_in: 3600, scope: scopes, ...token });
    }
    throw new Error('unexpected_network');
  };
  pending = await prepareSignIn({ hostId: 'urn:uuid:00000000-0000-4000-8000-000000000000', redirectUri, fetcher, registration: savedRegistration });
  const callback = new URL(redirectUri);
  callback.search = new URLSearchParams({ state: pending.state, code: 'synthetic-code', client_id: 'oaiapp_fixture' }).toString();
  return { pending, callback, calls };
}

describe('own SIWC authorization', () => {
  it('binds dynamic registration to this listener with fresh state, nonce and S256', async () => {
    const one = await fixture(), two = await fixture();
    const url = one.pending.authorizationUrl;
    expect(url.origin + url.pathname).toBe(`${issuer}/api/accounts/authorize`);
    expect(Object.fromEntries(url.searchParams)).toMatchObject({ client_id: 'dynamic_agent_client', redirect_uri: redirectUri,
      agent_name_hint: 'Ambient Assistant Feasibility', resource: 'https://api.openai.com/v1', scope: scopes,
      response_type: 'code', code_challenge_method: 'S256', state: one.pending.state, nonce: one.pending.nonce });
    expect(url.searchParams.get('code_challenge')).toMatch(/^[\w-]{43}$/);
    expect(one.pending.state).not.toBe(two.pending.state);
    expect(one.pending.nonce).not.toBe(two.pending.nonce);
    expect(one.calls).toHaveLength(1);
  });
  it('uses the issued client ID and exact redirect/resource/PKCE; retains only in-memory access', async () => {
    const f = await fixture();
    const session = await exchangeCallback(f.pending, f.callback);
    const request = f.calls.find(c => c.url === tokenUrl);
    expect(request.options.method).toBe('POST');
    expect(Object.fromEntries(new URLSearchParams(request.options.body))).toMatchObject({ client_id: 'oaiapp_fixture',
      code: 'synthetic-code', redirect_uri: redirectUri, resource: 'https://api.openai.com/v1',
      grant_type: 'authorization_code', code_verifier: f.pending.verifier });
    expect(session).toMatchObject({ accessToken: 'SYNTHETIC_ACCESS', refreshToken: 'SYNTHETIC_REFRESH',
      expiresAt: expect.any(Number), grantedScopes: scopes.split(' '), nonce: f.pending.nonce, registration });
    expect(f.calls.filter(c => c.url === tokenUrl)).toHaveLength(1);
    expect(f.calls.filter(c => c.url === jwksUrl)).toHaveLength(1);
    for (const call of f.calls) expect(call.options.redirect).toBe('error');
  });
  it.each(['state', 'code', 'client_id'])('rejects missing or duplicate %s before code exchange', async key => {
    for (const duplicate of [false, true]) {
      const f = await fixture();
      if (duplicate) f.callback.searchParams.append(key, f.callback.searchParams.get(key));
      else f.callback.searchParams.delete(key);
      await expect(exchangeCallback(f.pending, f.callback)).rejects.toThrow();
      expect(f.calls).toHaveLength(1);
    }
  });
  it('rejects wrong state, wrong callback host/path, placeholder and user denial without exchanging code', async () => {
    for (const mutate of [u => u.searchParams.set('state', 'other'), u => { u.hostname = 'localhost'; },
      u => { u.pathname = '/callback'; }, u => u.searchParams.set('client_id', 'dynamic_agent_client'),
      u => u.searchParams.set('error', 'access_denied')]) {
      const f = await fixture(); mutate(f.callback);
      await expect(exchangeCallback(f.pending, f.callback)).rejects.toThrow();
      expect(f.calls).toHaveLength(1);
    }
  });
  it.each([{ aud: 'wrong-client' }, { iss: 'https://attacker.invalid' }, { exp: 1 }, { nonce: 'wrong-nonce' }])(
    'rejects invalid ID-token claims %j', async claims => {
      const f = await fixture({ claims });
      await expect(exchangeCallback(f.pending, f.callback)).rejects.toThrow();
      expect(f.calls.filter(c => c.url === tokenUrl)).toHaveLength(1);
    });
  it('cryptographically rejects a modified ID-token signature', async () => {
    const f = await fixture({ mutateSignature: true });
    await expect(exchangeCallback(f.pending, f.callback)).rejects.toThrow();
    expect(f.calls.some(c => c.url === jwksUrl)).toBe(true);
  });
  it.each([{ id_token: undefined }, { expires_in: 0 }, { token_type: 'other' }])('rejects missing permission/token validity %j', async token => {
      const f = await fixture({ token });
      await expect(exchangeCallback(f.pending, f.callback)).rejects.toThrow();
    });
  it('keeps the verified registration when the first grant omitted its refresh token', async () => {
    const f = await fixture({ token: { refresh_token: undefined } });
    expect(await exchangeCallback(f.pending, f.callback)).toEqual({ registration, blockedReason: 'refresh_token_not_granted' });
  });
  it('persists only a verified binding when plan permission was not granted', async () => {
    for (const scope of ['openid profile email resource.invoke', scopes.replace('resource.invoke', '')]) {
      const f = await fixture({ token: { scope } });
      expect(await exchangeCallback(f.pending, f.callback)).toEqual({ registration, blockedReason: 'subscription_scope_not_granted' });
    }
  });
  it('reauthorizes the single saved registration without requiring an ID-token hint', async () => {
    const f = await fixture({ savedRegistration: registration });
    expect(f.pending.authorizationUrl.searchParams.get('client_id')).toBe(registration.client_id);
    expect(f.pending.authorizationUrl.searchParams.has('agent_name_hint')).toBe(false);
    expect(f.pending.authorizationUrl.searchParams.has('id_token_hint')).toBe(false);
    f.callback.searchParams.delete('client_id');
    expect(await exchangeCallback(f.pending, f.callback)).toMatchObject({ registration, accessToken: 'SYNTHETIC_ACCESS' });
  });
  it('rejects a different callback client or verified subject instead of replacing the binding', async () => {
    const wrongClient = await fixture({ savedRegistration: registration });
    wrongClient.callback.searchParams.set('client_id', 'oaiapp_other');
    await expect(exchangeCallback(wrongClient.pending, wrongClient.callback)).rejects.toThrow();
    expect(wrongClient.calls).toHaveLength(1);
    const wrongSubject = await fixture({ savedRegistration: registration, claims: { sub: 'other-subject' } });
    await expect(exchangeCallback(wrongSubject.pending, wrongSubject.callback)).rejects.toThrow();
  });
  it('rejects a malformed saved registration before contacting the provider', async () => {
    await expect(fixture({ savedRegistration: { ...registration, issuer: 'https://other.invalid' } })).rejects.toThrow();
  });
  it('rejects present but malformed registrations before discovery instead of re-registering', async () => {
    for (const registration of [null, false, 0, '', [], {}]) {
      let calls = 0;
      await expect(prepareSignIn({ hostId: 'urn:uuid:00000000-0000-4000-8000-000000000000', redirectUri, registration,
        fetcher: async () => { calls++; return Response.json(metadata); } })).rejects.toThrow();
      expect(calls).toBe(0);
    }
  });
  it.each([{ issuer: 'https://attacker.invalid' }, { token_endpoint: 'https://attacker.invalid/token' },
    { jwks_uri: 'https://attacker.invalid/keys' }, { id_token_signing_alg_values_supported: ['none'] }])(
    'rejects untrusted metadata %j', async discovery => {
      await expect(fixture({ discovery })).rejects.toThrow();
    });
});

const persistedSession = () => ({ access_token: 'SYNTHETIC_ACCESS', refresh_token: 'SYNTHETIC_REFRESH',
  expires_at_ms: Date.now() + 300_000, granted_scopes: scopes.split(' '), nonce: 'original-nonce', registration });

function memoryStore(session = persistedSession(), { fail = {} } = {}) {
  const calls = [];
  return { calls, get session() { return session; },
    async read(hostId, binding) { calls.push(['read', hostId, binding]); if (fail.read) throw new Error('SYNTHETIC_STORE_FAILURE'); return session; },
    async add(hostId, binding, value) { calls.push(['add', hostId, binding, value]); if (fail.add) throw new Error('SYNTHETIC_STORE_FAILURE'); session = value; },
    async update(hostId, binding, value) { calls.push(['update', hostId, binding, value]); if (fail.update) throw new Error('SYNTHETIC_STORE_FAILURE'); session = value; },
    async delete(hostId, binding) { calls.push(['delete', hostId, binding]); if (fail.delete) throw new Error('SYNTHETIC_STORE_FAILURE'); session = null; },
  };
}

async function refreshFixture({ token = {}, idTokenClaims = {}, includeIDToken = false, mutateSignature = false } = {}) {
  const calls = [];
  const fetcher = async (url, options) => {
    calls.push({ url: String(url), options });
    if (String(url).endsWith('/.well-known/openid-configuration')) return Response.json(metadata);
    if (String(url) === jwksUrl) return Response.json({ keys: [publicJwk] });
    if (String(url) === tokenUrl) {
      let idToken;
      if (includeIDToken) {
        idToken = await new SignJWT(idTokenClaims).setProtectedHeader({ alg: 'RS256', kid: 'fixture' })
          .setIssuer(idTokenClaims.iss ?? issuer).setAudience(idTokenClaims.aud ?? registration.client_id)
          .setSubject(idTokenClaims.sub ?? registration.subject).setIssuedAt()
          .setExpirationTime(idTokenClaims.exp ?? Math.floor(Date.now() / 1000) + 300).sign(privateKey);
        if (mutateSignature) { const pieces = idToken.split('.'); pieces[2] = (pieces[2][0] === 'A' ? 'B' : 'A') + pieces[2].slice(1); idToken = pieces.join('.'); }
      }
      return Response.json({ token_type: 'Bearer', access_token: 'ROTATED_ACCESS', refresh_token: 'ROTATED_REFRESH',
        expires_in: 3600, ...(idToken ? { id_token: idToken } : {}), ...token });
    }
    throw new Error('unexpected_network');
  };
  return { fetcher, calls };
}

describe('persisted SIWC session lifecycle', () => {
  const hostId = 'urn:uuid:00000000-0000-4000-8000-000000000000';

  it('reuses a valid bound Keychain session without opening the browser', async () => {
    const store = memoryStore(); let browserCalls = 0;
    const result = await authenticateSession({ hostId, registration, store, signIn: async () => { browserCalls++; } });
    expect(result).toMatchObject({ accessToken: 'SYNTHETIC_ACCESS', registration });
    expect(browserCalls).toBe(0);
    expect(store.calls.map(call => call[0])).toEqual(['read']);
  });

  it('deletes an expiring refresh token before one rotation and stores its replacement before returning', async () => {
    const session = persistedSession(); session.expires_at_ms = Date.now() + 1_000;
    const store = memoryStore(session), order = [];
    const replacement = { accessToken: 'ROTATED_ACCESS', refreshToken: 'ROTATED_REFRESH', expiresAt: Date.now() + 60_000,
      grantedScopes: scopes.split(' '), nonce: session.nonce, registration };
    const result = await authenticateSession({ hostId, registration, store,
      refresh: async prior => { expect(store.session).toBeNull(); order.push(['refresh', prior.refreshToken]); return replacement; },
      signIn: async () => { throw new Error('browser_must_not_open'); } });
    expect(store.calls.map(call => call[0])).toEqual(['read', 'delete', 'add']);
    expect(order).toEqual([['refresh', 'SYNTHETIC_REFRESH']]);
    expect(store.session).toMatchObject({ access_token: 'ROTATED_ACCESS', refresh_token: 'ROTATED_REFRESH', registration });
    expect(result.accessToken).toBe('ROTATED_ACCESS');
  });

  it('does not send refresh or open a browser when deleting the old token fails', async () => {
    const session = persistedSession(); session.expires_at_ms = 1;
    const store = memoryStore(session, { fail: { delete: true } }); let refreshCalls = 0, browserCalls = 0;
    const result = await authenticateSession({ hostId, registration, store,
      refresh: async () => { refreshCalls++; }, signIn: async () => { browserCalls++; } });
    expect(result).toMatchObject({ blockedReason: 'session_storage_failed' });
    expect(refreshCalls).toBe(0); expect(browserCalls).toBe(0);
    expect(store.calls.map(call => call[0])).toEqual(['read', 'delete']);
  });

  it('does not return a rotated access token when saving the replacement fails', async () => {
    const session = persistedSession(); session.expires_at_ms = 1;
    const store = memoryStore(session, { fail: { add: true } }); let refreshCalls = 0, browserCalls = 0;
    const result = await authenticateSession({ hostId, registration, store,
      refresh: async prior => { refreshCalls++; return { accessToken: 'ROTATED_ACCESS', refreshToken: 'ROTATED_REFRESH',
        expiresAt: Date.now() + 60_000, grantedScopes: prior.grantedScopes, nonce: prior.nonce, registration }; },
      signIn: async () => { browserCalls++; } });
    expect(result).toMatchObject({ blockedReason: 'session_storage_failed' });
    expect(refreshCalls).toBe(1); expect(browserCalls).toBe(0);
    expect(store.calls.map(call => call[0])).toEqual(['read', 'delete', 'add']);
    expect(JSON.stringify(result)).not.toContain('ROTATED_ACCESS');
  });

  it('does not retry or reuse the old credential after refresh transport loss', async () => {
    const session = persistedSession(); session.expires_at_ms = 1;
    const store = memoryStore(session); let refreshCalls = 0, browserCalls = 0;
    const result = await authenticateSession({ hostId, registration, store,
      refresh: async () => { refreshCalls++; throw new Error('SYNTHETIC_PRIVATE_REFRESH_ERROR'); },
      signIn: async () => { browserCalls++; } });
    expect(result).toMatchObject({ blockedReason: 'session_refresh_failed_reauthentication_required' });
    expect(refreshCalls).toBe(1); expect(browserCalls).toBe(0);
    expect(store.calls.map(call => call[0])).toEqual(['read', 'delete']);
    expect(store.session).toBeNull();
    expect(JSON.stringify(result)).not.toContain('SYNTHETIC_PRIVATE_REFRESH_ERROR');
  });

  it.each([
    { refreshToken: '' },
    { grantedScopes: ['openid', 'profile'] },
    { registration: { ...registration, subject: 'other-subject' } },
  ])('does not save or return an unusable refresh replacement %j', async delta => {
    const session = persistedSession(); session.expires_at_ms = 1;
    const store = memoryStore(session);
    const result = await authenticateSession({ hostId, registration, store,
      refresh: async prior => ({ ...prior, accessToken: 'ROTATED_ACCESS', refreshToken: 'ROTATED_REFRESH',
        expiresAt: Date.now() + 60_000, ...delta }),
      signIn: async () => { throw new Error('browser_must_not_open'); } });
    expect(result).toMatchObject({ blockedReason: 'session_refresh_failed_reauthentication_required' });
    expect(store.calls.map(call => call[0])).toEqual(['read', 'delete']);
    expect(JSON.stringify(result)).not.toContain('ROTATED_ACCESS');
  });

  it('stops on a Keychain read failure instead of opening the browser', async () => {
    const store = memoryStore(null, { fail: { read: true } }); let browserCalls = 0;
    const result = await authenticateSession({ hostId, registration, store, signIn: async () => { browserCalls++; } });
    expect(result).toMatchObject({ blockedReason: 'session_storage_failed' });
    expect(browserCalls).toBe(0);
  });

  it('persists a verified first browser session before returning it for model discovery', async () => {
    const store = memoryStore(null);
    const fresh = { accessToken: 'SYNTHETIC_ACCESS', refreshToken: 'SYNTHETIC_REFRESH', expiresAt: Date.now() + 60_000,
      grantedScopes: scopes.split(' '), nonce: 'original-nonce', registration };
    const result = await authenticateSession({ hostId, registration: undefined, store, signIn: async () => fresh });
    expect(store.calls.map(call => call[0])).toEqual(['add']);
    expect(store.calls[0][1]).toBe(hostId);
    expect(store.calls[0][2]).toEqual(registration);
    expect(store.calls[0][3]).toMatchObject({ access_token: 'SYNTHETIC_ACCESS', refresh_token: 'SYNTHETIC_REFRESH',
      granted_scopes: scopes.split(' '), expires_at_ms: expect.any(Number), nonce: 'original-nonce', registration });
    expect(result.accessToken).toBe('SYNTHETIC_ACCESS');
  });

  it('does not return a first browser session when the Keychain write fails', async () => {
    const store = memoryStore(null, { fail: { add: true } });
    const fresh = { accessToken: 'SYNTHETIC_ACCESS', refreshToken: 'SYNTHETIC_REFRESH', expiresAt: Date.now() + 60_000,
      grantedScopes: scopes.split(' '), nonce: 'original-nonce', registration };
    const result = await authenticateSession({ hostId, registration: undefined, store, signIn: async () => fresh });
    expect(result).toMatchObject({ registration, blockedReason: 'session_storage_failed' });
    expect(JSON.stringify(result)).not.toContain('SYNTHETIC_ACCESS');
    expect(store.calls.map(call => call[0])).toEqual(['add']);
  });

  it('rotates through the issued client and resource only, accepting absent optional scope and ID token', async () => {
    const f = await refreshFixture();
    const current = { accessToken: 'OLD_ACCESS', refreshToken: 'OLD_REFRESH', expiresAt: 1,
      grantedScopes: scopes.split(' '), nonce: 'original-nonce', registration };
    const result = await refreshSession(current, { fetcher: f.fetcher });
    const request = f.calls.find(call => call.url === tokenUrl);
    const body = Object.fromEntries(new URLSearchParams(request.options.body));
    expect(body).toMatchObject({ client_id: registration.client_id, grant_type: 'refresh_token', refresh_token: 'OLD_REFRESH',
      resource: 'https://api.openai.com/v1' });
    expect(body).not.toHaveProperty('scope');
    expect(body).not.toHaveProperty('client_secret');
    expect(body).not.toHaveProperty('code_verifier');
    expect(result).toMatchObject({ accessToken: 'ROTATED_ACCESS', refreshToken: 'ROTATED_REFRESH',
      grantedScopes: current.grantedScopes, nonce: 'original-nonce', registration, expiresAt: expect.any(Number) });
    expect(f.calls.filter(call => call.url === tokenUrl)).toHaveLength(1);
  });

  it.each([
    { idTokenClaims: { sub: 'other-subject' } },
    { idTokenClaims: { nonce: 'other-nonce' } },
    { idTokenClaims: { iss: 'https://attacker.invalid' } },
    { idTokenClaims: { aud: 'other-client' } },
    { idTokenClaims: { aud: [registration.client_id, 'other-client'] } },
    { idTokenClaims: { aud: [registration.client_id, 'other-client'], azp: 'other-client' } },
    { idTokenClaims: { exp: Math.floor(Date.now() / 1000) - 1 } },
    { mutateSignature: true },
  ])('rejects an invalid optional ID token during refresh %j', async options => {
    const f = await refreshFixture({ ...options, includeIDToken: true });
    await expect(refreshSession({ accessToken: 'OLD', refreshToken: 'OLD_REFRESH', expiresAt: 1,
      grantedScopes: scopes.split(' '), nonce: 'original-nonce', registration }, { fetcher: f.fetcher })).rejects.toThrow();
  });

  it('intersects refreshed scopes with previously verified permissions and requires a replacement refresh token', async () => {
    const reduced = await refreshFixture({ token: { scope: 'openid profile resource.invoke' } });
    await expect(refreshSession({ accessToken: 'OLD', refreshToken: 'OLD_REFRESH', expiresAt: 1,
      grantedScopes: scopes.split(' '), nonce: 'original-nonce', registration }, { fetcher: reduced.fetcher })).rejects.toThrow();
    const missingRotation = await refreshFixture({ token: { refresh_token: undefined } });
    await expect(refreshSession({ accessToken: 'OLD', refreshToken: 'OLD_REFRESH', expiresAt: 1,
      grantedScopes: scopes.split(' '), nonce: 'original-nonce', registration }, { fetcher: missingRotation.fetcher })).rejects.toThrow();
  });
  it('accepts a signed refresh ID token without a nonce and keeps the original nonce', async () => {
    const f = await refreshFixture({ includeIDToken: true });
    expect(await refreshSession({ refreshToken: 'OLD_REFRESH', grantedScopes: scopes.split(' '),
      nonce: 'original-nonce', registration }, { fetcher: f.fetcher })).toMatchObject({ nonce: 'original-nonce', registration });
    expect(f.calls.some(call => call.url === jwksUrl)).toBe(true);
  });
  it('rejects malformed stored identity before reading credentials or opening the browser', async () => {
    const store = memoryStore(); let browserCalls = 0;
    expect(await authenticateSession({ hostId, registration: null, store, signIn: async () => { browserCalls++; } }))
      .toMatchObject({ blockedReason: 'session_binding_invalid' });
    expect(store.calls).toHaveLength(0); expect(browserCalls).toBe(0);
  });
  it('rejects a cached session for a different subject without refresh or browser fallback', async () => {
    const cached = persistedSession(); cached.registration = { ...registration, subject: 'different-subject' };
    const store = memoryStore(cached); let browserCalls = 0, refreshCalls = 0;
    expect(await authenticateSession({ hostId, registration, store, refresh: async () => { refreshCalls++; },
      signIn: async () => { browserCalls++; } })).toMatchObject({ blockedReason: 'session_binding_invalid' });
    expect(browserCalls).toBe(0); expect(refreshCalls).toBe(0);
  });
});

import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { describe, expect, it, vi } from 'vitest';
import { keychainSessionStore } from './keychain.mjs';

const hostId = 'urn:uuid:00000000-0000-4000-8000-000000000000';
const registration = { client_id: 'oaiapp_fixture', issuer: 'https://auth.openai.com', subject: 'synthetic-subject' };
const session = { access_token: 'SYNTHETIC_ACCESS_SECRET', refresh_token: 'SYNTHETIC_REFRESH_SECRET', expires_at_ms: 123456,
  granted_scopes: ['openid', 'resource.invoke', 'chatgpt.tokens.use.direct'], nonce: 'SYNTHETIC_NONCE', registration };

function subprocessFixture(respond) {
  const requests = [];
  const spawnProcess = vi.fn((binary, args, options) => {
    const child = new EventEmitter();
    child.stdin = new PassThrough(); child.stdout = new PassThrough(); child.stderr = new PassThrough();
    child.kill = vi.fn();
    let input = '';
    child.stdin.on('data', chunk => { input += chunk; });
    child.stdin.on('end', () => {
      requests.push({ binary, args, options, request: JSON.parse(input) });
      const response = respond(requests.at(-1).request);
      queueMicrotask(() => { child.stdout.end(JSON.stringify(response) + '\n'); child.emit('close', 0); });
    });
    return child;
  });
  return { requests, spawnProcess };
}

describe('native Keychain helper boundary', () => {
  it('sends bounded JSON through pipes with a minimal environment and returns a found session in memory', async () => {
    const f = subprocessFixture(() => ({ version: 1, outcome: 'found', session }));
    const store = keychainSessionStore({ spawnProcess: f.spawnProcess, helperPath: '/tmp/keychain-helper' });
    expect(await store.read(hostId, registration)).toEqual(session);
    const call = f.requests[0];
    expect(call.request).toMatchObject({ version: 1, operation: 'read', account: expect.any(String) });
    expect(call.request).not.toHaveProperty('session');
    expect(call.options.stdio).toEqual(['pipe', 'pipe', 'ignore']);
    expect(Object.keys(call.options.env).sort()).toEqual(['HOME', 'PATH']);
    expect(call.request.account).toMatch(/^v1:[a-f0-9]{64}$/);
  });

  it('uses a bounded deterministic account key even for the largest issued client ID', async () => {
    const f = subprocessFixture(() => ({ version: 1, outcome: 'not_found' }));
    const store = keychainSessionStore({ spawnProcess: f.spawnProcess });
    const binding = { ...registration, client_id: 'a'.repeat(512) };
    await store.read(hostId, binding); await store.read(hostId, binding);
    expect(f.requests[0].request.account.length).toBeLessThanOrEqual(512);
    expect(f.requests[1].request.account).toBe(f.requests[0].request.account);
  });

  it('uses fixed add/update/delete operations and does not include secrets in argv or env', async () => {
    const f = subprocessFixture(request => ({ version: 1, outcome: request.operation === 'delete' ? 'deleted' : 'stored' }));
    const store = keychainSessionStore({ spawnProcess: f.spawnProcess, helperPath: '/tmp/keychain-helper' });
    await store.add(hostId, registration, session);
    await store.update(hostId, registration, session);
    await store.delete(hostId, registration);
    expect(f.requests.map(call => call.request.operation)).toEqual(['add', 'update', 'delete']);
    expect(f.requests[0].request.session).toEqual(session);
    for (const call of f.requests) {
      expect(call.args).toEqual([]);
      expect(JSON.stringify(call.args) + JSON.stringify(call.options.env)).not.toContain('SYNTHETIC_');
    }
  });

  it('does not expose helper errors or oversized output to the caller', async () => {
    const f = subprocessFixture(() => ({ version: 1, outcome: 'error', error_code: 'keychain_failure',
      status_code: -25293, detail: 'SYNTHETIC_PRIVATE_DETAIL' }));
    const store = keychainSessionStore({ spawnProcess: f.spawnProcess, helperPath: '/tmp/keychain-helper' });
    await expect(store.read(hostId, registration)).rejects.toMatchObject({ reason: 'keychain_helper_failed' });
    const oversized = subprocessFixture(() => ({ version: 1, outcome: 'found', session: { ...session,
      refresh_token: 'x'.repeat(70 * 1024) } }));
    const oversizedStore = keychainSessionStore({ spawnProcess: oversized.spawnProcess, helperPath: '/tmp/keychain-helper' });
    await expect(oversizedStore.read(hostId, registration)).rejects.toMatchObject({ reason: 'helper_response_too_large' });
  });

  it('waits for helper process close after timeout requests termination', async () => {
    const child = new EventEmitter();
    child.stdin = new PassThrough(); child.stdout = new PassThrough(); child.stderr = new PassThrough();
    const order = [];
    child.kill = vi.fn(() => { order.push('kill'); return true; });
    const store = keychainSessionStore({ helperPath: '/tmp/keychain-helper', timeoutMilliseconds: 5,
      spawnProcess: () => child });
    vi.useFakeTimers();
    try {
      const pending = store.read(hostId, registration).then(() => order.push('resolved'), () => order.push('rejected'));
      await vi.advanceTimersByTimeAsync(5);
      expect(order).toEqual(['kill']);
      order.push('close'); child.emit('close', null, 'SIGKILL');
      await pending;
      expect(order).toEqual(['kill', 'close', 'rejected']);
    } finally { vi.useRealTimers(); }
  });

  it.each(['overflow', 'stdin_error', 'abort'])('waits for close after %s before rejecting', async event => {
    const child = new EventEmitter();
    child.stdin = new PassThrough(); child.stdout = new PassThrough(); child.kill = vi.fn(() => true);
    const controller = new AbortController(); let settled = false;
    const store = keychainSessionStore({ spawnProcess: () => child, signal: controller.signal });
    const pending = store.read(hostId, registration).catch(error => { settled = true; return error; });
    child.emit('spawn');
    if (event === 'overflow') child.stdout.write(Buffer.alloc(65 * 1024));
    if (event === 'stdin_error') child.stdin.emit('error', new Error('SYNTHETIC_PRIVATE_DETAIL'));
    if (event === 'abort') controller.abort();
    try {
      await Promise.resolve();
      expect(child.kill).toHaveBeenCalledOnce(); expect(settled).toBe(false);
    } finally { child.emit('close', null, 'SIGKILL'); }
    const error = await pending;
    expect(settled).toBe(true);
    expect(String(error)).not.toContain('SYNTHETIC_PRIVATE_DETAIL');
  });
});

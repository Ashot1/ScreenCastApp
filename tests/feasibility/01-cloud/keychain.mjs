import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const helperPath = fileURLToPath(new URL('./native-keychain/.build/release/keychain-helper', import.meta.url));
const maximumMessageBytes = 64 * 1024;
const maximumSessionBytes = 48 * 1024;
const timeoutMilliseconds = 120_000;

export class KeychainSessionError extends Error {
  constructor(reason) {
    super('keychain_helper_failed');
    this.name = 'KeychainSessionError';
    this.reason = reason;
  }
}

function keychainAccount(hostId, registration) {
  if (typeof hostId !== 'string' || !/^urn:uuid:[0-9a-f-]{36}$/.test(hostId) ||
      !registration || typeof registration !== 'object' || registration.issuer !== 'https://auth.openai.com' ||
      typeof registration.client_id !== 'string' || !/^[A-Za-z0-9_-]{1,512}$/.test(registration.client_id) ||
      typeof registration.subject !== 'string' || !registration.subject || registration.subject.length > 1024) {
    throw new KeychainSessionError('invalid_binding');
  }
  return 'v1:' + createHash('sha256').update(JSON.stringify([hostId, registration.client_id, registration.issuer, registration.subject])).digest('hex');
}

function decodeResponse(data) {
  if (!data.length || data.length > maximumMessageBytes) throw new KeychainSessionError('invalid_helper_response');
  let response;
  try { response = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(data)); } catch { throw new KeychainSessionError('invalid_helper_response'); }
  if (!response || response.version !== 1 || typeof response.outcome !== 'string') {
    throw new KeychainSessionError('invalid_helper_response');
  }
  if (response.outcome === 'error') throw new KeychainSessionError('keychain_helper_failed');
  return response;
}

function invokeHelper(request, options = {}) {
  const { helperPath: executable = helperPath, spawnProcess = spawn } = options;
  if (options.signal?.aborted) return Promise.reject(new KeychainSessionError('helper_cancelled'));
  let encoded;
  try { encoded = Buffer.from(JSON.stringify(request)); }
  catch { return Promise.reject(new KeychainSessionError('invalid_request')); }
  if (encoded.length > maximumMessageBytes) return Promise.reject(new KeychainSessionError('request_too_large'));

  return new Promise((resolve, reject) => {
    let child;
    let settled = false;
    let spawned = false;
    let pendingFailure;
    let size = 0;
    const output = [];
    const finish = (error, response) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      options.signal?.removeEventListener('abort', onAbort);
      if (error) reject(error);
      else resolve(response);
    };
    const stopAndWait = reason => {
      if (settled || pendingFailure) return;
      pendingFailure = new KeychainSessionError(reason);
      child.kill('SIGKILL');
    };
    const onAbort = () => stopAndWait('helper_cancelled');
    // Keychain may show a first-use access prompt; keep the active probe lock until the child exits.
    const timer = setTimeout(() => stopAndWait('helper_timeout'), options.timeoutMilliseconds ?? timeoutMilliseconds);

    try {
      const environment = { PATH: '/usr/bin:/bin' };
      if (typeof process.env.HOME === 'string' && process.env.HOME.startsWith('/')) environment.HOME = process.env.HOME;
      child = spawnProcess(executable, [], { cwd: '/', env: environment, stdio: ['pipe', 'pipe', 'ignore'] });
    } catch {
      finish(new KeychainSessionError('helper_unavailable'));
      return;
    }
    child.stdout.on('data', chunk => {
      size += chunk.byteLength;
      if (size > maximumMessageBytes) {
        stopAndWait('helper_response_too_large');
      } else output.push(chunk);
    });
    child.on('spawn', () => { spawned = true; });
    child.on('error', () => {
      if (!spawned) finish(new KeychainSessionError('helper_unavailable'));
      else stopAndWait('helper_io_failed');
    });
    child.stdin.on('error', () => stopAndWait('helper_io_failed'));
    child.on('close', code => {
      if (settled) return;
      if (pendingFailure) {
        finish(pendingFailure);
        return;
      }
      if (code !== 0) {
        finish(new KeychainSessionError('helper_failed'));
        return;
      }
      try { finish(undefined, decodeResponse(Buffer.concat(output))); }
      catch (error) { finish(error instanceof KeychainSessionError ? error : new KeychainSessionError('invalid_helper_response')); }
    });
    options.signal?.addEventListener('abort', onAbort, { once: true });
    if (options.signal?.aborted) onAbort();
    if (!pendingFailure) child.stdin.end(encoded);
  });
}

export function keychainSessionStore(options = {}) {
  async function request(operation, hostId, registration, session) {
    const message = { version: 1, operation, account: keychainAccount(hostId, registration), ...(session ? { session } : {}) };
    return invokeHelper(message, options);
  }
  return {
    async read(hostId, registration) {
      const response = await request('read', hostId, registration);
      if (response.outcome === 'not_found') return null;
      if (response.outcome !== 'found' || !response.session || typeof response.session !== 'object' ||
          Buffer.byteLength(JSON.stringify(response.session)) > maximumSessionBytes) {
        throw new KeychainSessionError('invalid_helper_response');
      }
      return response.session;
    },
    async add(hostId, registration, session) {
      const response = await request('add', hostId, registration, session);
      if (response.outcome !== 'stored') throw new KeychainSessionError('keychain_helper_failed');
    },
    async update(hostId, registration, session) {
      const response = await request('update', hostId, registration, session);
      if (response.outcome !== 'stored') throw new KeychainSessionError('keychain_helper_failed');
    },
    async delete(hostId, registration) {
      const response = await request('delete', hostId, registration);
      if (response.outcome !== 'deleted') throw new KeychainSessionError('keychain_helper_failed');
    },
  };
}

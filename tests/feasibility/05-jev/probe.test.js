import { afterEach, describe, it, expect, vi } from 'vitest';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { initializeLedger, budgetStatus, reserve, settle } from './budget.mjs';
import { buildRequest, validateResponse, preflight, runAttempt } from './probe.mjs';

describe('Jev synthetic request boundary', () => {
  // Losing the reference image or leaking ground truth invalidates the real experiment.
  it('prepares a pinned multipart request containing both images without ground truth', async () => {
    const prepared = buildRequest('P-image');
    expect(prepared).toMatchObject({ url: 'https://api.typesafe.ai/v1/systemone' });
    const request = JSON.parse(await prepared.body.get('request').text());
    expect(request.model).toBe('jev-1.14.0');
    expect(Object.keys(request.assets)).toHaveLength(2);
    expect(prepared.body.get('scene-photo').type).toBe('image/png');
    expect(prepared.body.get('reference-photo').size).toBeGreaterThan(0);
    expect(request.questions.location.type).toBe('box');
    expect(request.questions.presence.type).toBe('noul');
    expect(request.questions.count.criteria).toHaveProperty('uncertain');
    expect(JSON.stringify(request)).not.toMatch(/red_circle_boxes|source_crop|100,120|sha256/);
  });

  it('refuses an unrecognised case instead of reading an arbitrary file', () => {
    expect(() => buildRequest('../../personal.png')).toThrow('unknown_case');
  });
});

const resources = [];
afterEach(async () => {
  for (const cleanup of resources.splice(0)) await cleanup();
  vi.restoreAllMocks();
});
const purpose = 'Check a specific unverified synthetic criterion';
const approved = { live_enabled: true, upper_bound_usd: '0.01', upper_bound_evidence: 'synthetic bound, not a live proof',
  account_terms_reference: 'synthetic test consent', presence_min: 0.9, unique_min: 0.9 };
function isolatedLedger(opening = '0') {
  const directory = mkdtempSync(join(tmpdir(), 'jev-boundary-test-'));
  resources.push(() => rmSync(directory, { force: true, recursive: true }));
  const path = join(directory, 'ledger.sqlite3');
  initializeLedger(path, opening, 'synthetic opening exposure');
  return path;
}
async function loopback(handler) {
  const server = createServer(handler);
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  resources.unshift(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
  return `http://127.0.0.1:${server.address().port}/v1/systemone`;
}

describe('offline admission and real loopback HTTP serialization', () => {
  it.each([
    ['0.789999999', null],
    ['0.79', 800000000],
    ['0.8', 810000000],
  ])('preflight warns at 80 percent including the new reserve with opening %s', (opening, exposure) => {
    const ledgerPath = isolatedLedger(opening);
    const result = preflight({ purpose, caseId: 'P-en', settings: approved, apiKey: 'test-only-key', ledgerPath });
    expect(result.status).toBe('ready');
    if (exposure === null) expect(result.warning).toBeNull();
    else expect(result.warning).toMatchObject({ kind: 'budget_warning_80_percent', cap_nanousd: 1000000000,
      committed_with_reservations_nanousd: exposure, basis: 'charges_and_unsettled_reservations_including_new_attempt' });
    expect(budgetStatus(ledgerPath).attempts).toHaveLength(0);
  });
  it('includes settled charges and older unknown attempts in the warning', () => {
    const ledgerPath = isolatedLedger('0.6');
    const paid = reserve(ledgerPath, 'earlier-task', '0.2', 'synthetic bound', { purpose, model: 'jev-1.14.0', endpoint: 'https://api.typesafe.ai/v1/systemone' });
    settle(ledgerPath, paid, '0.1', 'synthetic invoice');
    reserve(ledgerPath, 'earlier-unknown', '0.09', 'synthetic bound', { purpose, model: 'jev-1.14.0', endpoint: 'https://api.typesafe.ai/v1/systemone' });
    expect(preflight({ purpose, caseId: 'P-en', settings: approved, apiKey: 'test-only-key', ledgerPath }).warning)
      .toMatchObject({ committed_with_reservations_nanousd: 800000000 });
  });
  it('emits one warning after committed reservation and before actual fetch', async () => {
    const ledgerPath = isolatedLedger('0.79');
    const events = [];
    const testURL = await loopback((req, res) => {
      events.push({ kind: 'received', exposure: budgetStatus(ledgerPath).committed_nanousd });
      res.writeHead(200, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(response()));
    });
    // Trace the real fetch, preserving the HTTP boundary rather than returning a fake response.
    const realFetch = globalThis.fetch;
    vi.spyOn(console, 'warn').mockImplementation(text => events.push({ ...JSON.parse(text),
      persisted_exposure: budgetStatus(ledgerPath).committed_nanousd }));
    vi.spyOn(globalThis, 'fetch').mockImplementation((...args) => { events.push({ kind: 'fetch' }); return realFetch(...args); });
    expect(preflight({ purpose, caseId: 'P-en', settings: approved, apiKey: 'test-only-key', ledgerPath }).status).toBe('ready');
    expect(events).toEqual([]);
    const result = await runAttempt({ purpose, caseId: 'P-en', settings: approved, apiKey: 'test-only-key', ledgerPath, testURL });
    expect(result.status).toBe('unreconciled');
    expect(events.map(item => item.kind)).toEqual(['budget_warning_80_percent', 'fetch', 'received']);
    expect(events[0]).toMatchObject({ committed_with_reservations_nanousd: 800000000, persisted_exposure: 800000000 });
    expect(events[2].exposure).toBe(800000000);
  });
  it('dry-runs without dispatch or inventing unknown opening spend', () => {
    expect(preflight({ purpose, caseId: 'P-ru', settings: {}, apiKey: '' })).toMatchObject({
      status: 'blocked', network_calls: 0, budget: { cap_nanousd: 1000000000, remaining_nanousd: null },
    });
    expect(preflight({ purpose, caseId: 'P-ru', settings: {}, apiKey: '' }).blockers).toEqual(expect.arrayContaining([
      'missing_api_key', 'live_disabled', 'unconfirmed_upper_bound', 'unconfirmed_account_terms', 'missing_or_invalid_shared_ledger',
    ]));
  });
  it('requires an explicit purpose for the next paid criterion', () => {
    const ledgerPath = isolatedLedger();
    expect(preflight({ caseId: 'P-en', settings: approved, apiKey: 'test-only-key', ledgerPath }).blockers)
      .toContain('missing_test_purpose');
    expect(budgetStatus(ledgerPath).attempts).toHaveLength(0);
  });
  it('blocks missing key, cost evidence and budget before a server can receive anything', async () => {
    let received = 0;
    const testURL = await loopback((req, res) => { received++; res.end('{}'); });
    const ledgerPath = isolatedLedger('1');
    for (const options of [
      { settings: approved, apiKey: '' },
      { settings: { ...approved, upper_bound_evidence: null }, apiKey: 'test-only-key' },
      { settings: approved, apiKey: 'test-only-key', purpose: '' },
      { settings: approved, apiKey: 'test-only-key' },
    ]) {
      expect(await runAttempt({ purpose, caseId: 'P-en', ledgerPath, testURL, ...options })).toMatchObject({ status: 'blocked' });
    }
    expect(received).toBe(0);
    expect(budgetStatus(ledgerPath).committed_nanousd).toBe(1000000000);
  });
  it('sends genuine multipart bytes only to loopback and keeps the successful attempt reserved until a bill is reconciled', async () => {
    const ledgerPath = isolatedLedger();
    let wire;
    const testURL = await loopback(async (req, res) => {
      const parts = await new Request('http://127.0.0.1', { method: req.method, headers: req.headers, body: req, duplex: 'half' }).formData();
      wire = { method: req.method, url: req.url, auth: req.headers.authorization,
        images: req.headers['typesafe-danger-zone-images'], request: JSON.parse(await parts.get('request').text()),
        sceneBytes: parts.get('scene-photo').size, referenceBytes: parts.get('reference-photo').size,
        alreadyReserved: budgetStatus(ledgerPath).committed_nanousd, audit: budgetStatus(ledgerPath).attempts[0] };
      res.writeHead(200, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(response()));
    });
    const result = await runAttempt({ purpose, caseId: 'P-image', settings: approved, apiKey: 'test-only-key', ledgerPath, testURL });
    expect(wire).toMatchObject({ method: 'POST', url: '/v1/systemone', auth: 'Bearer test-only-key', images: 'true',
      sceneBytes: 4102, referenceBytes: 1341, alreadyReserved: 10000000, audit: { purpose, model: 'jev-1.14.0', endpoint: testURL, charged: null, reserved: 10000000 }, request: { model: 'jev-1.14.0' } });
    expect(result).toMatchObject({ status: 'unreconciled', classification: { kind: 'candidate', highlight_permitted: false } });
    expect(budgetStatus(ledgerPath).committed_nanousd).toBe(10000000);
    expect(JSON.stringify(budgetStatus(ledgerPath).attempts)).not.toMatch(/test-only-key|Authorization|iVBORw0KGgo/);
  });
  it.each(['http_error', 'invalid_json', 'oversize', 'timeout', 'redirect'])('retains the reserve without retry for %s', async mode => {
    const ledgerPath = isolatedLedger();
    let received = 0;
    const testURL = await loopback((req, res) => {
      received++;
      if (mode === 'timeout') return;
      if (mode === 'redirect') { res.writeHead(302, { Location: 'https://api.typesafe.ai/v1/systemone' }); res.end(); return; }
      if (mode === 'http_error') res.writeHead(429);
      res.end(mode === 'oversize' ? 'x'.repeat(65537) : 'not-json');
    });
    expect(await runAttempt({ purpose, caseId: 'P-en', settings: approved, apiKey: 'test-only-key', ledgerPath, testURL, timeoutMs: 100 }))
      .toMatchObject({ status: 'unknown' });
    expect(received).toBe(1);
    expect(budgetStatus(ledgerPath).committed_nanousd).toBe(10000000);
  });
  it('forbids alternative live ledgers and non-loopback test endpoints', async () => {
    const ledgerPath = isolatedLedger();
    expect(await runAttempt({ purpose, caseId: 'P-en', settings: approved, apiKey: 'test-only-key', ledgerPath }))
      .toMatchObject({ status: 'blocked' });
    expect(await runAttempt({ purpose, caseId: 'P-en', settings: approved, apiKey: 'test-only-key', ledgerPath, testURL: 'https://example.com' }))
      .toMatchObject({ status: 'blocked' });
    expect(budgetStatus(ledgerPath).committed_nanousd).toBe(0);
  });
});

const thresholds = { presence_min: 0.9, unique_min: 0.9 };
function response() {
  return { model: 'jev-1.14.0', answers: {
    presence: { type: 'noul', noul: 0.99 },
    count: { type: 'choice', choice: 'one', probabilities: { none: 0, one: 0.99, several: 0, uncertain: 0.01 }, confidence: 0.99 },
    location: { type: 'box', box: { left: 0.125, top: 0.2, right: 0.275, bottom: 0.4 }, confidence: 0.99 },
  }, usage: { input_tokens: 100, output_tokens: 21 } };
}

describe('untrusted Jev response boundary', () => {
  const context = buildRequest('P-en').context;
  const classify = data => validateResponse(data, context, context.scene_sha256, thresholds);
  it('maps a candidate to original pixels without claiming a permitted highlight', () => {
    expect(classify(response())).toMatchObject({ kind: 'candidate', source_box: [100, 120, 220, 240], highlight_permitted: false });
  });
  it('maps an actual crop response using its source offset', () => {
    const cropped = buildRequest('C-en').context;
    const data = response();
    data.answers.location.box = { left: 0.05, top: 0.1, right: 0.35, bottom: 0.4 };
    expect(validateResponse(data, cropped, cropped.scene_sha256, thresholds).source_box).toEqual([100, 120, 220, 240]);
  });
  it('ignores even a confident box when a separate presence check says absent', () => {
    const data = response();
    data.answers.presence.noul = 0.01;
    data.answers.count = { type: 'choice', choice: 'none', probabilities: { none: 0.99, one: 0, several: 0, uncertain: 0.01 }, confidence: 0.99 };
    expect(classify(data)).toMatchObject({ kind: 'absent', source_box: null });
  });
  it('rejects ambiguous targets and conflicting presence evidence', () => {
    const data = response();
    data.answers.count = { type: 'choice', choice: 'several', probabilities: { none: 0, one: 0, several: 0.99, uncertain: 0.01 }, confidence: 0.99 };
    expect(classify(data).kind).toBe('ambiguous');
    data.answers.presence.noul = 0.01;
    expect(classify(data)).toMatchObject({ kind: 'uncertain', source_box: null });
  });
  it('requires calibrated thresholds and rejects a stale snapshot', () => {
    expect(validateResponse(response(), context, context.scene_sha256, null).kind).toBe('unverified');
    expect(validateResponse(response(), context, 'changed', thresholds).kind).toBe('stale');
  });
  it.each([NaN, Infinity, true, -0.1, 1.1])('rejects invalid external coordinates %s', value => {
    const data = response(); data.answers.location.box.left = value;
    expect(classify(data)).toMatchObject({ kind: 'invalid', source_box: null });
  });
  it('rejects inverted edges, wrong versions, invalid probabilities and unknown usage', () => {
    const data = response(); data.answers.location.box.left = 0.9;
    expect(classify(data).kind).toBe('invalid');
    const version = response(); version.model = 'jev-latest';
    expect(classify(version).kind).toBe('invalid');
    const probabilities = response(); probabilities.answers.count.probabilities.one = 0.5;
    expect(classify(probabilities).kind).toBe('invalid');
    const unknown = response(); delete unknown.usage.output_tokens;
    expect(classify(unknown).usage).toBeNull();
  });
});

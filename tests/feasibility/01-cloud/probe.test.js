import { afterEach, describe, expect, it } from 'vitest';
import { mkdtemp, mkdir, readFile, writeFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { connect } from 'node:net';
import { join } from 'node:path';
import { buildRequest, runProbe, createCallbackListener } from './probe.mjs';
import { authenticateSession } from './auth.mjs';
const registration = { client_id: 'oaiapp_fixture', issuer: 'https://auth.openai.com', subject: 'synthetic-subject' };
const fixture = { id: 'text-none', observation: 'Synthetic test-1: no useful action.', evidence_ids: ['test-1'] };
const approvedFixture = JSON.parse(await readFile(new URL('cases.json', import.meta.url), 'utf8')).find(item => item.id === 'text-none');
const payload = { kind: 'none', message: null, rationale: 'No unfinished task.', evidence_ids: ['test-1'] };
const completedFor = (configuration = {}) => {
  const model = Object.hasOwn(configuration, 'model') ? configuration.model : 'gpt-6-luna';
  const serviceTier = Object.hasOwn(configuration, 'service_tier') ? configuration.service_tier : 'priority';
  const effort = Object.hasOwn(configuration, 'reasoning_effort') ? configuration.reasoning_effort : 'low';
  return `data: ${JSON.stringify({ type: 'response.completed', response: { status: 'completed',
    ...(model === undefined ? {} : { model }), ...(serviceTier === undefined ? {} : { service_tier: serviceTier }),
  ...(effort === undefined ? {} : { reasoning: { effort } }),
  output: [{ type: 'message', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: JSON.stringify(payload) }] }],
  usage: { input_tokens: 50, output_tokens: 20, total_tokens: 70 } } })}\n\n`;
};
const completed = completedFor();
const dirs = [];
const privateHttpDetail = 'SYNTHETIC_PRIVATE_HTTP_DETAIL';
afterEach(async () => { for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true }); });
async function setup({ confirm = 'RUN', response = () => new Response(completed, { headers: { 'content-type': 'text/event-stream' } }),
  signIn, fetcher: injectedFetcher, model = 'gpt-6-luna', reasoningEffort = 'low', models = [{ slug: 'gpt-6-luna', visibility: 'list' }] } = {}) {
  const dir = await mkdtemp(join(tmpdir(), 'ambient-cloud-01-')); dirs.push(dir);
  const calls = [], timeline = [];
  const options = { fixture, model, reasoningEffort, localDir: join(dir, 'local'), evidenceDir: join(dir, 'evidence'),
    authenticate: signIn ?? (async () => { timeline.push('auth'); return { accessToken: 'SYNTHETIC_ACCESS', expiresAt: Date.now() + 300_000, registration }; }),
    confirm: async message => { timeline.push('confirm'); expect(message).toContain('https://chatgpt.com/'); return confirm; },
    fetcher: injectedFetcher ?? (async (url, init) => {
      calls.push({ url: String(url), init }); timeline.push(init.method ?? 'GET');
      if (String(url).endsWith('/models')) return Response.json({ models });
      return response(init);
    }),
  };
  return { dir, options, calls, timeline };
}

async function expectUnknownHttpFailure(f, result, status) {
  expect(result).toMatchObject({ status: 'http_error', http_status: status, post_dispatch: 'sent',
    usage: null, usage_status: 'unknown', model_request_count_not_assumed: true,
    diagnostic: { stage: 'model_request', reason: status >= 200 && status < 300 ? 'unexpected_content_type' : 'http_error', http_status: status } });
  const attempt = await readFile(join(f.options.evidenceDir, result.attempt_id + '.json'), 'utf8');
  const ledger = await readFile(join(f.options.evidenceDir, 'unresolved.json'), 'utf8');
  expect(JSON.stringify(result) + attempt + ledger).not.toContain(privateHttpDetail);
  expect(JSON.parse(ledger)).toMatchObject({ status: 'http_error', post_dispatch: 'sent', usage_status: 'unknown' });
  expect(await runProbe(f.options)).toMatchObject({ status: 'blocked', reason: 'unresolved_prior_usage' });
  expect(await readFile(join(f.options.evidenceDir, 'unresolved.json'), 'utf8')).toBe(ledger);
  expect(f.calls.filter(call => call.init.method === 'POST')).toHaveLength(1);
}

describe('one synthetic subscription probe', () => {
  it('persists reconstructed output metadata without text while retaining unknown usage and Fast mismatch', async () => {
    const event = JSON.parse(completed.slice(6).trim());
    event.response.output = []; event.response.service_tier = 'default'; event.response.usage = null;
    const text = JSON.stringify({ kind: 'none', message: null, rationale: 'PRIVATE_DELTA_TEXT', evidence_ids: [] });
    const delta = { type: 'response.output_text.delta', output_index: 0, content_index: 0, item_id: 'PRIVATE_ITEM_ID', delta: text };
    const done = { type: 'response.output_item.done', output_index: 0, item: { type: 'message', role: 'assistant', status: 'completed', id: 'PRIVATE_ID',
      content: [{ type: 'output_text', text }] } };
    const wire = `data: ${JSON.stringify(delta)}\n\ndata: ${JSON.stringify(done)}\n\ndata: ${JSON.stringify(event)}\n\n`;
    const f = await setup({ response: () => new Response(wire, { headers: { 'content-type': 'text/event-stream' } }) });
    const options = { ...f.options, fixture: approvedFixture, diagnosticMode: true };
    const result = await runProbe(options);
    expect(result).toMatchObject({ status: 'configuration_mismatch', usage_status: 'unknown', proposal: { kind: 'none' }, output_source: 'completed_items',
      validation_metadata: { terminal_output_count: 0, configuration: { service_tier: 'default' } } });
    expect(result.stream_event_metadata).not.toHaveProperty('candidate');
    const attempt = await readFile(join(options.evidenceDir, result.attempt_id + '.json'), 'utf8');
    const ledger = await readFile(join(options.evidenceDir, 'unresolved.json'), 'utf8');
    expect(JSON.stringify(result) + attempt + ledger).not.toContain('PRIVATE');
    expect(JSON.parse(ledger).stream_event_metadata).toEqual(result.stream_event_metadata);
    expect(await runProbe(options)).toMatchObject({ status: 'blocked', reason: 'unresolved_prior_usage' });
    expect(f.calls.filter(call => call.init.method === 'POST')).toHaveLength(1);
  });
  it.each(['priority', 'default'])('keeps the Fast guard for actual-shaped completed items with tier %s', async service_tier => {
    const event = JSON.parse(completed.slice(6).trim()); event.response.output = []; event.response.service_tier = service_tier;
    const message = { type: 'message', role: 'assistant', status: 'completed', id: 'PRIVATE_ID',
      content: [{ type: 'output_text', text: JSON.stringify({ ...payload, rationale: 'PRIVATE_TEXT' }) }] };
    const events = [{ type: 'response.output_item.done', output_index: 0, item: { type: 'reasoning' } },
      { type: 'response.output_item.done', output_index: 1, item: message }, event];
    const f = await setup({ response: () => new Response(events.map(item => `data: ${JSON.stringify(item)}\n\n`).join(''), { headers: { 'content-type': 'text/event-stream' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result).toMatchObject({ status: service_tier === 'priority' ? 'completed' : 'configuration_mismatch', output_source: 'completed_items',
      proposal: { kind: 'none' }, usage_status: 'reported' });
    const saved = await readFile(join(f.options.evidenceDir, result.attempt_id + '.json'), 'utf8');
    expect(JSON.stringify(result) + saved).not.toContain('PRIVATE');
  });
  it.each([
    [undefined, 'missing'], [null, 'null'], ['fast', 'fast'], ['priority', 'priority'],
    ['auto', 'auto'], ['default', 'default'], ['flex', 'flex'], ['standard', 'standard'],
    ['PRIVATE_TIER', 'other_string'], [1, 'other_type'],
  ])('preserves the closed raw tier state %j before privacy projection', async (tier, state) => {
    const f = await setup({ response: () => new Response(completedFor({ service_tier: tier }), { headers: { 'content-type': 'text/event-stream' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result.validation_metadata).toMatchObject({ configuration: { model: 'matching', service_tier: state, reasoning_effort: 'matching' } });
    expect(result.status).toBe(['fast', 'priority'].includes(tier) ? 'completed' : 'configuration_mismatch');
    const saved = await readFile(join(f.options.evidenceDir, result.attempt_id + '.json'), 'utf8');
    expect(JSON.parse(saved).validation_metadata).toEqual(result.validation_metadata);
    expect(JSON.stringify(result) + saved).not.toContain('PRIVATE_TIER');
  });
  it.each([
    [{ model: undefined }, 'model', 'missing'], [{ model: null }, 'model', 'null'], [{ model: 'PRIVATE_MODEL' }, 'model', 'different'],
    [{ reasoning_effort: undefined }, 'reasoning_effort', 'missing'], [{ reasoning_effort: null }, 'reasoning_effort', 'null'],
    [{ reasoning_effort: 'high' }, 'reasoning_effort', 'different'],
  ])('distinguishes missing/null/different requested configuration %j', async (configuration, field, state) => {
    const f = await setup({ response: () => new Response(completedFor(configuration), { headers: { 'content-type': 'text/event-stream' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result.validation_metadata.configuration[field]).toBe(state);
    expect(JSON.stringify(result.validation_metadata)).not.toContain('PRIVATE');
  });
  it('retains exact rejection metadata while removing invalid output text before persistence', async () => {
    const f = await setup({ response: () => new Response(completed.replace('No unfinished task.', 'PRIVATE_OUTPUT').replace('"status":"completed","content"', '"status":"PRIVATE_STATUS","content"'), { headers: { 'content-type': 'text/event-stream' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result).toMatchObject({ status: 'completed', usage_status: 'reported', proposal: { kind: 'invalid' },
      validation_metadata: { proposal_rejection: 'message_status', terminal_fields: { output: 'array', service_tier: 'string' },
        message_fields: { status: 'string' } } });
    const saved = await readFile(join(f.options.evidenceDir, result.attempt_id + '.json'), 'utf8');
    expect(JSON.stringify(result) + saved).not.toContain('PRIVATE');
  });
  it.each([undefined, ' Text/Event-Stream ; charset=UTF-8'])('accepts strict completed SSE with direct-route Content-Type %j', async contentType => {
    const f = await setup({ response: () => new Response(new TextEncoder().encode(completed), {
      headers: contentType === undefined ? {} : { 'content-type': contentType },
    }) });
    const result = await runProbe(f.options);
    expect(result).toMatchObject({ status: 'completed', usage_status: 'reported',
      response_metadata: { http_status: 200, media_type: contentType === undefined ? 'absent' : 'sse', body_present: true, body_read_outcome: 'terminal_event' } });
    expect(f.calls.find(call => call.init.method === 'POST').init.headers.Accept).toBe('text/event-stream');
    expect(f.calls.find(call => call.url.endsWith('/models')).init.headers).not.toHaveProperty('Accept');
  });
  it('does not turn absent-header JSON into a completed stream', async () => {
    const f = await setup({ response: () => new Response(new TextEncoder().encode(JSON.stringify({ response: { status: 'completed' }, usage: { input_tokens: 1 } }))) });
    const result = await runProbe(f.options);
    expect(result).toMatchObject({ status: 'incomplete', post_dispatch: 'sent', usage_status: 'unknown',
      response_metadata: { http_status: 200, media_type: 'absent', body_present: true, body_read_outcome: 'eof' } });
    expect(f.calls.filter(call => call.init.method === 'POST')).toHaveLength(1);
    expect(await readdir(f.options.evidenceDir)).toContain('unresolved.json');
  });
  it('rejects an explicit non-SSE MIME even when the body is valid SSE', async () => {
    const f = await setup({ response: () => new Response(completed, { headers: { 'content-type': 'application/json' } }) });
    const result = await runProbe(f.options);
    expect(result).toMatchObject({ status: 'http_error', http_status: 200, usage_status: 'unknown',
      diagnostic: { stage: 'model_request', reason: 'unexpected_content_type', http_status: 200 },
      response_metadata: { http_status: 200, media_type: 'json', body_present: true, body_read_outcome: 'invalid_json' } });
  });
  it('retains received headers when stream reader acquisition fails', async () => {
    const f = await setup({ response: () => {
      const response = new Response(new TextEncoder().encode(completed));
      response.body.getReader();
      return response;
    } });
    const result = await runProbe(f.options);
    expect(result).toMatchObject({ status: 'interrupted', post_dispatch: 'sent',
      response_metadata: { http_status: 200, media_type: 'absent', body_present: true, body_read_outcome: 'read_error', bytes_observed: 0 } });
  });
  it.each([
    ['complete', () => Response.json({ detail: 'Unsupported parameter: model' }, { status: 400 })],
    ['empty', () => new Response(null, { status: 400 })],
    ['limit_exceeded', () => new Response(new Uint8Array(16385), { status: 400 })],
    ['invalid_utf8', () => new Response(new Uint8Array([255]), { status: 400 })],
    ['invalid_json', () => new Response(new TextEncoder().encode('{'), { status: 400 })],
    ['read_error', () => new Response(new ReadableStream({ start(controller) { controller.error(new Error(privateHttpDetail)); } }), { status: 400 })],
  ])('records closed diagnostic body outcome %s before final persistence', async (outcome, response) => {
    const f = await setup({ response });
    const result = await runProbe(f.options);
    expect(result.response_metadata).toMatchObject({ http_status: 400, body_read_outcome: outcome, byte_limit: 16384, bytes_observed: expect.any(Number) });
    expect(JSON.parse(await readFile(join(f.options.evidenceDir, result.attempt_id + '.json'), 'utf8')).response_metadata).toEqual(result.response_metadata);
    expect(JSON.parse(await readFile(join(f.options.evidenceDir, 'unresolved.json'), 'utf8')).response_metadata).toEqual(result.response_metadata);
  });
  it('limits temporary diagnostics to an exact approved synthetic fixture before authentication', async () => {
    const f = await setup();
    expect(await runProbe({ ...f.options, diagnosticMode: true })).toMatchObject({ status: 'blocked', reason: 'diagnostic_case_not_approved' });
    expect(f.timeline).toEqual([]);
  });
  it('captures bounded opt-in explanations and fixed-format correlation IDs without private values', async () => {
    const explanation = 'Unsupported parameter: service_tier';
    const f = await setup({ response: () => Response.json({ detail: explanation }, { status: 400,
      headers: { 'x-request-id': 'req_0123456789abcdef0123456789abcdef' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result.diagnostic).toMatchObject({ request_id: 'req_0123456789abcdef0123456789abcdef', provider_explanation: explanation });
    expect(result.model_output_omitted).toBe(true);
    const saved = JSON.stringify(result) + await readFile(join(f.options.evidenceDir, result.attempt_id + '.json'), 'utf8') + await readFile(join(f.options.evidenceDir, 'unresolved.json'), 'utf8');
    for (const secret of ['SYNTHETIC_ACCESS', 'synthetic-subject', 'oaiapp_fixture', 'private.example', 'person@example.com', 'abcdefghijklmnopqrstuvwxyz', approvedFixture.observation]) expect(saved).not.toContain(secret);
    expect(result.diagnostic.provider_explanation.length).toBeLessThanOrEqual(512);
  });
  it('keeps expanded diagnostics off by default', async () => {
    const f = await setup({ response: () => Response.json({ detail: 'Remove this parameter.' }, { status: 400, headers: { 'x-request-id': 'req_0123456789abcdef' } }) });
    const result = await runProbe(f.options);
    expect(result.diagnostic).not.toHaveProperty('provider_explanation');
    expect(result.diagnostic).not.toHaveProperty('request_id');
  });
  it.each(['person%40example.com', 'SYN THET IC_ACCESS', 'Unsupported field. '.repeat(100), 'Unsupported parameter: secret_field'])('omits free text and transformed values %j', async detail => {
    const f = await setup({ response: () => Response.json({ detail }, { status: 400 }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result.diagnostic).not.toHaveProperty('provider_explanation');
  });
  it('rejects a case-transformed known secret as a correlation ID', async () => {
    const f = await setup({ signIn: async () => ({ accessToken: 'REQ_0123456789ABCDEF', expiresAt: Date.now() + 300_000, registration }),
      response: () => Response.json({ detail: 'Unsupported parameter: model' }, { status: 400, headers: { 'x-request-id': 'req_0123456789abcdef' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result.diagnostic).not.toHaveProperty('request_id');
  });
  it('keeps a configuration mismatch while removing unrecognized provider configuration strings', async () => {
    const f = await setup({ response: () => new Response(completedFor({ model: 'PRIVATE_MODEL_OUTPUT', service_tier: 'PRIVATE_TIER', reasoning_effort: 'PRIVATE_EFFORT' }), { headers: { 'content-type': 'text/event-stream' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result).toMatchObject({ status: 'invalid', returned_configuration: { model: null, service_tier: null, reasoning_effort: null } });
    expect(JSON.stringify(result) + await readFile(join(f.options.evidenceDir, result.attempt_id + '.json'), 'utf8')).not.toMatch(/PRIVATE_MODEL_OUTPUT|PRIVATE_TIER|PRIVATE_EFFORT/);
  });
  it('computes model mismatch before omitting an arbitrary returned model', async () => {
    const f = await setup({ response: () => new Response(completedFor({ model: 'PRIVATE_MODEL_OUTPUT' }), { headers: { 'content-type': 'text/event-stream' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result).toMatchObject({ status: 'configuration_mismatch', returned_configuration: { model: null, service_tier: 'priority', reasoning_effort: 'low' } });
  });
  it('omits arbitrary discovered model names in diagnostic mode', async () => {
    const f = await setup({ models: [{ slug: 'PRIVATE_MODEL_NAME', visibility: 'list' }] });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result).toMatchObject({ status: 'blocked', reason: 'model_not_available' });
    expect(result).not.toHaveProperty('available_models');
  });
  it('keeps only a safe correlation ID after diagnostic overflow and retains unknown usage', async () => {
    const f = await setup({ response: () => new Response('x'.repeat(16385), { status: 400, headers: { 'x-request-id': 'req_0123456789abcdef' } }) });
    f.options = { ...f.options, fixture: approvedFixture, diagnosticMode: true };
    const result = await runProbe(f.options);
    expect(result.diagnostic.request_id).toBe('req_0123456789abcdef');
    expect(result.diagnostic).not.toHaveProperty('provider_explanation');
    await expectUnknownHttpFailure(f, result, 400);
  });
  it.each(['unsafe\u0000content', 'небезопасный текст', { secret: 'private' }])('omits unsupported explanation %j and an arbitrary request ID', async detail => {
    const f = await setup({ response: () => Response.json({ detail }, { status: 400, headers: { 'x-request-id': 'SYNTHETIC_ACCESS' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result.diagnostic).not.toHaveProperty('provider_explanation');
    expect(result.diagnostic).not.toHaveProperty('request_id');
  });
  it('removes model text before temporary diagnostic results are returned or saved', async () => {
    const f = await setup({ response: () => new Response(completed.replace('No unfinished task.', 'PRIVATE_MODEL_OUTPUT'), { headers: { 'content-type': 'text/event-stream' } }) });
    const result = await runProbe({ ...f.options, fixture: approvedFixture, diagnosticMode: true });
    expect(result).toMatchObject({ status: 'completed', model_output_omitted: true, proposal: { kind: 'none' } });
    expect(result.proposal).toEqual({ kind: 'none' });
    expect(JSON.stringify(result) + await readFile(join(f.options.evidenceDir, result.attempt_id + '.json'), 'utf8')).not.toContain('PRIVATE_MODEL_OUTPUT');
  });
  it('builds an inert structured request with Luna low effort and Fast mode by default', () => {
    const request = buildRequest({ ...fixture, image: true, max_output_tokens: 16 }, 'gpt-6-luna', 'data:image/png;base64,AA==', 'low');
    expect(request).toMatchObject({ model: 'gpt-6-luna', reasoning: { effort: 'low' }, service_tier: 'priority', tools: [], store: false, stream: true,
      text: { format: { type: 'json_schema', strict: true } } });
    expect(Object.keys(request).sort()).toEqual(['input', 'instructions', 'model', 'reasoning', 'service_tier', 'store', 'stream', 'text', 'tools']);
    expect(request.input[0].content).toContainEqual({ type: 'input_image', image_url: 'data:image/png;base64,AA==' });
    expect(JSON.stringify(request)).not.toContain('expected');
  });
  it('allows an explicit high reasoning effort without changing the requested model or tier', () => {
    const request = buildRequest(fixture, 'gpt-6-luna', undefined, 'high');
    expect(request).toMatchObject({ model: 'gpt-6-luna', reasoning: { effort: 'high' }, service_tier: 'priority' });
  });
  it.each([
    { model: 'gpt-6.1-sol', reasoningEffort: 'low' },
    { model: 'gpt-6-luna', reasoningEffort: 'medium' },
  ])('rejects unsupported model or effort before auth and filesystem changes (%j)', async selection => {
    const f = await setup(selection);
    expect(await runProbe(f.options)).toMatchObject({ status: 'blocked', reason: 'invalid_case_or_model' });
    expect(f.timeline).toEqual([]); expect(f.calls).toHaveLength(0);
    await expect(readdir(f.options.localDir)).rejects.toMatchObject({ code: 'ENOENT' });
  });
  it('authenticates first, asks for human confirmation, then sends exactly one Responses POST', async () => {
    const f = await setup();
    const result = await runProbe(f.options);
    expect(f.timeline).toEqual(['auth', 'confirm', 'GET', 'POST']);
    expect(f.calls.filter(c => c.init.method === 'POST')).toHaveLength(1);
    expect(result).toMatchObject({ status: 'completed', proposal: { kind: 'none' }, usage_status: 'reported',
      requested_configuration: { model: 'gpt-6-luna', reasoning_effort: 'low', service_tier: 'priority' },
      returned_configuration: { model: 'gpt-6-luna', service_tier: 'priority', reasoning_effort: 'low' },
      credit_guard: { source: 'user_reported', confirmed: true }, elapsed_ms: expect.any(Number) });
    const posted = JSON.parse(f.calls.find(c => c.init.method === 'POST').init.body);
    expect(posted).toMatchObject({ model: 'gpt-6-luna', reasoning: { effort: 'low' }, service_tier: 'priority' });
    for (const c of f.calls) { expect(c.init.redirect).toBe('error'); expect(c.init.headers.Authorization).toBe('Bearer SYNTHETIC_ACCESS'); }
    const saved = await readFile(join(f.options.evidenceDir, result.attempt_id + '.json'), 'utf8');
    expect(saved).not.toMatch(/SYNTHETIC_ACCESS|SYNTHETIC_REFRESH|access_token|id_token/);
    expect(await readdir(f.options.localDir)).toEqual(['installation.json']);
    expect(await readdir(f.options.evidenceDir)).not.toContain('unresolved.json');
  });
  it.each(['', 'yes', 'credits on'])('refuses unconfirmed credit guard %j before any inference or discovery', async confirm => {
    const f = await setup({ confirm });
    expect(await runProbe(f.options)).toMatchObject({ status: 'blocked', reason: 'credit_confirmation_required' });
    expect(f.timeline).toEqual(['auth', 'confirm']); expect(f.calls).toHaveLength(0);
  });
  it('stops after failed authentication and refuses expired access before models/POST', async () => {
    const denied = await setup({ signIn: async () => { throw new Error('SYNTHETIC_PRIVATE_ERROR'); } });
    await expect(runProbe(denied.options)).rejects.toThrow(); expect(denied.calls).toHaveLength(0);
    const expired = await setup({ signIn: async () => ({ accessToken: 'secret', expiresAt: 1, registration }) });
    expect(await runProbe(expired.options)).toMatchObject({ status: 'blocked', reason: 'expired_session' });
    expect(expired.calls).toHaveLength(0);
  });
  it('retains and passes only the verified registration for a later process sign-in', async () => {
    const f = await setup({ confirm: '' });
    await runProbe(f.options);
    let selected;
    await runProbe({ ...f.options, authenticate: async options => { selected = options.registration; return { registration, blockedReason: 'subscription_scope_not_granted' }; } });
    expect(selected).toEqual(registration);
    const saved = JSON.parse(await readFile(join(f.options.localDir, 'installation.json'), 'utf8'));
    expect(saved).toMatchObject({ registration, registration_pending: false });
    expect(JSON.stringify(saved)).not.toContain('SYNTHETIC_ACCESS');
    expect(f.calls).toHaveLength(0);
  });
  it('blocks fresh registration after a lost or failed initial callback until human reconciliation', async () => {
    const f = await setup({ signIn: async () => { throw new Error('lost callback'); } });
    await expect(runProbe(f.options)).rejects.toThrow();
    expect(await runProbe(f.options)).toMatchObject({ status: 'blocked', reason: 'registration_requires_reconciliation' });
    expect(f.calls).toHaveLength(0);
  });
  it('keeps registration pending when sign-in stops without a verified registration', async () => {
    const f = await setup({ signIn: async () => ({ blockedReason: 'sign_in_failed' }) });
    expect(await runProbe(f.options)).toMatchObject({ status: 'blocked', reason: 'sign_in_failed' });
    const installation = JSON.parse(await readFile(join(f.options.localDir, 'installation.json'), 'utf8'));
    expect(installation).toMatchObject({ registration_pending: true });
    expect(installation).not.toHaveProperty('registration');
    expect(f.calls).toHaveLength(0);
  });
  it('does not reach model discovery when saving the first session fails', async () => {
    const f = await setup({ signIn: async () => ({ registration, blockedReason: 'session_storage_failed' }) });
    expect(await runProbe(f.options)).toMatchObject({ status: 'blocked', reason: 'session_storage_failed' });
    expect(f.calls).toHaveLength(0);
    const installation = JSON.parse(await readFile(join(f.options.localDir, 'installation.json'), 'utf8'));
    expect(installation).toMatchObject({ registration, registration_pending: false });
    expect(JSON.stringify(installation)).not.toContain('SYNTHETIC_ACCESS');
  });
  it('saves login before discovery and reuses it after a model-list failure while asking RUN each time', async () => {
    let cached = null, browserCalls = 0, confirmations = 0;
    const events = [];
    const store = {
      async read() { events.push('read'); return cached; },
      async add(hostId, binding, session) { events.push('add'); cached = session; },
      async delete() { events.push('delete'); cached = null; },
    };
    const f = await setup({ fetcher: async () => {
      events.push('GET'); expect(cached?.refresh_token).toBe('SYNTHETIC_REFRESH');
      return Response.json({ error: { message: 'SYNTHETIC_PRIVATE_ERROR' } }, { status: 503 });
    } });
    f.options.confirm = async () => { confirmations++; return 'RUN'; };
    f.options.authenticate = options => authenticateSession({ ...options, store,
      signIn: async () => { browserCalls++; events.push('browser'); return {
        accessToken: 'SYNTHETIC_ACCESS', refreshToken: 'SYNTHETIC_REFRESH', expiresAt: Date.now() + 300_000,
        grantedScopes: ['openid', 'resource.invoke', 'chatgpt.tokens.use.direct'], nonce: 'synthetic-nonce', registration,
      }; } });
    const first = await runProbe(f.options);
    const second = await runProbe(f.options);
    expect(first).toMatchObject({ status: 'pre_post_failed', post_dispatch: 'not_sent' });
    expect(second).toMatchObject({ status: 'pre_post_failed', post_dispatch: 'not_sent' });
    expect(browserCalls).toBe(1); expect(confirmations).toBe(2);
    expect(events).toEqual(['browser', 'add', 'GET', 'read', 'GET']);
    const installation = await readFile(join(f.options.localDir, 'installation.json'), 'utf8');
    expect(installation + JSON.stringify([first, second])).not.toMatch(/SYNTHETIC_ACCESS|SYNTHETIC_REFRESH|SYNTHETIC_PRIVATE_ERROR/);
  });
  it('does not silently replace a missing/hidden model', async () => {
    const f = await setup({ models: [{ slug: 'gpt-6-luna', visibility: 'hide' }, { slug: 'other', visibility: 'list' }] });
    expect(await runProbe(f.options)).toMatchObject({ status: 'blocked', reason: 'model_not_available', available_models: ['other'] });
    expect(f.calls).toHaveLength(1);
  });
  it('accepts a visible model in a catalog with synthetic metadata above 512 KiB', async () => {
    const models = Array.from({ length: 9 }, (_, index) => ({
      slug: index === 0 ? 'gpt-6-luna' : `synthetic-model-${index}`, visibility: 'list',
      description: 'Synthetic capability metadata. '.repeat(2100),
    }));
    expect(Buffer.byteLength(JSON.stringify({ models }))).toBeGreaterThan(524288);
    const f = await setup({ models });
    expect(await runProbe(f.options)).toMatchObject({ status: 'completed', usage_status: 'reported' });
    expect(f.calls.map(call => call.init.method ?? 'GET')).toEqual(['GET', 'POST']);
  });
  it('reports safe model-discovery diagnostics for transport, HTTP, invalid JSON and wrong response shapes', async () => {
    const cases = [
      { label: 'transport', fetcher: async () => { throw new TypeError('SYNTHETIC_PRIVATE_DETAIL', { cause: Object.assign(new Error(), { code: 'UND_ERR_CONNECT_TIMEOUT' }) }); },
        expected: { stage: 'model_discovery', reason: 'transport_error', system_code: 'UND_ERR_CONNECT_TIMEOUT' } },
      { label: 'timeout', fetcher: async () => { throw new DOMException('SYNTHETIC_PRIVATE_DETAIL', 'TimeoutError'); },
        expected: { stage: 'model_discovery', reason: 'timeout' } },
      { label: 'abort', fetcher: async () => { throw new DOMException('SYNTHETIC_PRIVATE_DETAIL', 'AbortError'); },
        expected: { stage: 'model_discovery', reason: 'cancelled' } },
      { label: 'HTTP', fetcher: async () => new Response(JSON.stringify({ error: { message: 'SYNTHETIC_PRIVATE_DETAIL' } }), { status: 401 }),
        expected: { stage: 'model_discovery', reason: 'http_error', http_status: 401, response_shape: 'error_object' } },
      { label: 'invalid JSON', fetcher: async () => new Response('SYNTHETIC_PRIVATE_DETAIL'),
        expected: { stage: 'model_discovery', reason: 'invalid_json', response_shape: 'non_json' } },
      { label: 'ordinary API schema', fetcher: async () => Response.json({ data: [{ id: 'SYNTHETIC_PRIVATE_DETAIL' }] }),
        expected: { stage: 'model_discovery', reason: 'invalid_model_list', response_shape: 'data_array' } },
      { label: 'null JSON', fetcher: async () => Response.json(null),
        expected: { stage: 'model_discovery', reason: 'invalid_model_list', response_shape: 'other_json' } },
    ];
    for (const item of cases) {
      const f = await setup({ fetcher: async (url, init) => {
        f.calls.push({ url: String(url), init }); f.timeline.push(init.method ?? 'GET');
        return item.fetcher(url, init);
      } });
      const result = await runProbe(f.options);
      expect(result).toMatchObject({ status: 'pre_post_failed', diagnostic: item.expected, post_dispatch: 'not_sent', usage_status: 'not_applicable' });
      expect(f.calls.filter(call => call.init.method === 'POST')).toHaveLength(0);
      expect(JSON.stringify(result)).not.toContain('SYNTHETIC_PRIVATE_DETAIL');
      await expect(readdir(f.options.evidenceDir)).rejects.toMatchObject({ code: 'ENOENT' });
    }
  });
  it.each([
    [2097152, 200, 'completed', undefined, 1, false],
    [2097152, 503, 'pre_post_failed', 'http_error', 0, false],
    [2097153, 200, 'pre_post_failed', 'response_too_large', 0, true],
    [2097153, 503, 'pre_post_failed', 'response_too_large', 0, true],
  ])('bounds a discovery body of %i bytes with HTTP %i', async (bytes, status, expectedStatus, reason, posts, cancelled) => {
    const prefix = Buffer.from(JSON.stringify(status === 200
      ? { models: [{ slug: 'gpt-6-luna', visibility: 'list', description: 'Synthetic metadata 🧪' }] }
      : { error: { message: 'SYNTHETIC_PRIVATE_DETAIL 🧪' } }));
    const body = Buffer.concat([prefix, Buffer.alloc(bytes - prefix.byteLength, 0x20)]);
    let offset = 0, wasCancelled = false;
    const stream = new ReadableStream({
      pull(controller) {
        if (offset === body.byteLength) { controller.close(); return; }
        const end = Math.min(offset + 65536, body.byteLength);
        controller.enqueue(body.subarray(offset, end)); offset = end;
      },
      cancel() { wasCancelled = true; },
    }, { highWaterMark: 0 });
    const f = await setup({ fetcher: async (url, init) => {
      f.calls.push({ url: String(url), init });
      return String(url).endsWith('/models') ? new Response(stream, { status })
        : new Response(completed, { headers: { 'content-type': 'text/event-stream' } });
    } });
    const result = await runProbe(f.options);
    expect(result.status).toBe(expectedStatus);
    if (reason) expect(result).toMatchObject({ post_dispatch: 'not_sent', usage_status: 'not_applicable',
      diagnostic: { stage: 'model_discovery', reason, http_status: status } });
    expect(f.calls.filter(call => call.init.method === 'POST')).toHaveLength(posts);
    expect(wasCancelled).toBe(cancelled);
    expect(JSON.stringify(result)).not.toContain('SYNTHETIC_PRIVATE_DETAIL');
  });
  it('reports evidence preparation failure before the Responses POST', async () => {
    const f = await setup();
    const evidenceFile = join(f.dir, 'evidence-is-a-file');
    f.options.evidenceDir = evidenceFile;
    f.options.confirm = async () => { await writeFile(evidenceFile, 'SYNTHETIC_PRIVATE_DETAIL'); return 'RUN'; };
    const result = await runProbe(f.options);
    expect(result).toMatchObject({ status: 'pre_post_failed', post_dispatch: 'not_sent', usage_status: 'not_applicable',
      diagnostic: { stage: 'evidence_preparation', reason: 'filesystem_error' } });
    expect(f.calls.filter(call => call.init.method === 'POST')).toHaveLength(0);
    expect(JSON.stringify(result)).not.toContain('SYNTHETIC_PRIVATE_DETAIL');
  });
  it('writes the attempt record before the unresolved marker and never leaves false unknown usage on a pre-POST write failure', async () => {
    const f = await setup({ fetcher: async (url, init) => {
      f.calls.push({ url: String(url), init }); f.timeline.push(init.method ?? 'GET');
      if (String(url).endsWith('/models')) {
        const bytes = new TextEncoder().encode(JSON.stringify({ models: [{ slug: 'gpt-6-luna', visibility: 'list' }] }));
        return new Response(new ReadableStream({ async pull(controller) {
          await mkdir(f.options.evidenceDir, { recursive: true });
          await mkdir(join(f.options.evidenceDir, 'unresolved.json'));
          controller.enqueue(bytes); controller.close();
        } }), { headers: { 'content-type': 'application/json' } });
      }
      return new Response(completed, { headers: { 'content-type': 'text/event-stream' } });
    } });
    const result = await runProbe(f.options);
    expect(result).toMatchObject({ status: 'pre_post_failed', post_dispatch: 'not_sent', usage_status: 'not_applicable',
      diagnostic: { stage: 'evidence_preparation', reason: 'filesystem_error' } });
    expect(f.calls.filter(call => call.init.method === 'POST')).toHaveLength(0);
    expect((await readdir(f.options.evidenceDir, { withFileTypes: true }))
      .filter(entry => entry.isFile() && entry.name.endsWith('.json'))).toHaveLength(1);
  });
  it('keeps the unresolved ledger and does not claim zero after evidence finalization fails post-POST', async () => {
    const f = await setup({ response: async () => {
      const record = JSON.parse(await readFile(join(f.options.evidenceDir, 'unresolved.json'), 'utf8'));
      const attemptPath = join(f.options.evidenceDir, record.attempt_id + '.json');
      await rm(attemptPath);
      await mkdir(attemptPath);
      return new Response(completed, { headers: { 'content-type': 'text/event-stream' } });
    } });
    const result = await runProbe(f.options);
    expect(result).toMatchObject({ status: 'evidence_error', diagnostic: { stage: 'evidence_finalization', reason: 'filesystem_error' },
      post_dispatch: 'sent', model_request_count_not_assumed: true, usage_status: 'reported',
      response_metadata: { http_status: 200, media_type: 'sse', body_present: true, body_read_outcome: 'terminal_event' } });
    expect(f.calls.filter(call => call.init.method === 'POST')).toHaveLength(1);
    expect(await readFile(join(f.options.evidenceDir, 'unresolved.json'), 'utf8')).toContain('dispatch_pending');
  });
  it('records explicit high effort and treats a provider tier downgrade as a failed configuration match while retaining usage', async () => {
    const f = await setup({ reasoningEffort: 'high', response: () => new Response(completedFor({ service_tier: 'default', reasoning_effort: 'high' }), { headers: { 'content-type': 'text/event-stream' } }) });
    const result = await runProbe(f.options);
    expect(result).toMatchObject({ status: 'configuration_mismatch', usage_status: 'reported',
      requested_configuration: { model: 'gpt-6-luna', reasoning_effort: 'high', service_tier: 'priority' },
      returned_configuration: { model: 'gpt-6-luna', service_tier: 'default', reasoning_effort: 'high' } });
    expect(f.calls.filter(c => c.init.method === 'POST')).toHaveLength(1);
    expect(await readdir(f.options.evidenceDir)).not.toContain('unresolved.json');
  });
  it('accepts the returned fast alias when requesting priority for Fast mode', async () => {
    const f = await setup({ response: () => new Response(completedFor({ service_tier: 'fast' }), { headers: { 'content-type': 'text/event-stream' } }) });
    expect(await runProbe(f.options)).toMatchObject({ status: 'completed', usage_status: 'reported',
      requested_configuration: { service_tier: 'priority' }, returned_configuration: { service_tier: 'fast', reasoning_effort: 'low' } });
  });
  it('requires the returned model and a confirmed Fast tier for a completed result', async () => {
    for (const returned of [{ model: undefined, service_tier: 'fast' }, { model: 'gpt-6-luna', service_tier: undefined }, { model: 'other', service_tier: 'fast' }]) {
      const f = await setup({ response: () => new Response(completedFor(returned), { headers: { 'content-type': 'text/event-stream' } }) });
      const result = await runProbe(f.options);
      expect(result.status).toBe('configuration_mismatch');
      expect(result.usage_status).toBe('reported');
      expect(f.calls.filter(c => c.init.method === 'POST')).toHaveLength(1);
    }
  });
  it('stores an absent returned effort as null and rejects an explicit effort mismatch', async () => {
    const omitted = await setup({ response: () => new Response(completedFor({ reasoning_effort: undefined }), { headers: { 'content-type': 'text/event-stream' } }) });
    expect(await runProbe(omitted.options)).toMatchObject({ status: 'completed', returned_configuration: { reasoning_effort: null } });
    const mismatch = await setup({ response: () => new Response(completedFor({ reasoning_effort: 'high' }), { headers: { 'content-type': 'text/event-stream' } }) });
    expect(await runProbe(mismatch.options)).toMatchObject({ status: 'configuration_mismatch', usage_status: 'reported',
      requested_configuration: { reasoning_effort: 'low' }, returned_configuration: { reasoning_effort: 'high' } });
  });
  it.each([
    'subscription_sharing_usage_limit_exceeded',
    'subscription_sharing_usage_unavailable',
    'subscription_sharing_unsupported_capability',
  ])('keeps only the allowed HTTP provider code %s and parameter', async code => {
    const f = await setup({ response: () => Response.json({ error: { code, param: 'service_tier',
      message: privateHttpDetail, type: privateHttpDetail }, detail: privateHttpDetail },
    { status: 400, headers: { 'x-request-id': privateHttpDetail } }) });
    const result = await runProbe(f.options);
    expect(result.diagnostic).toEqual({ stage: 'model_request', reason: 'http_error', http_status: 400,
      response_shape: 'error_object', provider_code: code, provider_param: 'service_tier' });
    await expectUnknownHttpFailure(f, result, 400);
  });
  it('keeps only the detail-only HTTP shape without disclosing private detail', async () => {
    const f = await setup({ response: () => Response.json({ detail: privateHttpDetail }, { status: 400 }) });
    const result = await runProbe(f.options);
    expect(result.diagnostic).toEqual({ stage: 'model_request', reason: 'http_error', http_status: 400,
      response_shape: 'detail_only', provider_code: 'provider_failure' });
    await expectUnknownHttpFailure(f, result, 400);
  });
  it.each(['model', 'service_tier', 'reasoning', 'reasoning.effort', 'tools', 'input', 'instructions',
    'store', 'stream', 'text', 'text.format'])('allows the fixed HTTP provider parameter %s', async param => {
    const f = await setup({ response: () => Response.json({ error: {
      code: 'subscription_sharing_unsupported_capability', param, message: privateHttpDetail,
    } }, { status: 400 }) });
    const result = await runProbe(f.options);
    expect(result.diagnostic.provider_param).toBe(param);
    await expectUnknownHttpFailure(f, result, 400);
  });
  it.each([privateHttpDetail, 'input.0.content', 'service_tier ', { field: privateHttpDetail }])(
    'generalizes unknown HTTP codes and omits non-whitelisted parameter %j', async param => {
      const f = await setup({ response: () => Response.json({ error: { code: privateHttpDetail, param,
        message: privateHttpDetail, type: privateHttpDetail }, detail: privateHttpDetail },
      { status: 400, headers: { 'x-request-id': privateHttpDetail } }) });
      const result = await runProbe(f.options);
      expect(result.diagnostic).toEqual({ stage: 'model_request', reason: 'http_error', http_status: 400,
        response_shape: 'error_object', provider_code: 'provider_failure' });
      await expectUnknownHttpFailure(f, result, 400);
    });
  it.each([privateHttpDetail, '{"error":', new Uint8Array([0xFF])])(
    'keeps only a non-JSON shape for invalid HTTP error body %j', async body => {
      const f = await setup({ response: () => new Response(body, { status: 400 }) });
      const result = await runProbe(f.options);
      expect(result.diagnostic).toEqual({ stage: 'model_request', reason: 'http_error', http_status: 400,
        response_shape: 'non_json' });
      await expectUnknownHttpFailure(f, result, 400);
    });
  it.each([16384, 16385])('bounds HTTP error diagnostics at %i decoded bytes', async bytes => {
    const prefix = Buffer.from(JSON.stringify({ error: { code: 'subscription_sharing_unsupported_capability',
      param: 'text.format', message: privateHttpDetail + ' 🧪' } }));
    const body = Buffer.concat([prefix, Buffer.alloc(bytes - prefix.byteLength, 0x20)]);
    let offset = 0, cancelled = false;
    const f = await setup({ response: () => new Response(new ReadableStream({
      pull(controller) {
        if (offset === body.byteLength) { controller.close(); return; }
        const end = Math.min(offset + 4096, body.byteLength);
        controller.enqueue(body.subarray(offset, end)); offset = end;
      },
      cancel() { cancelled = true; },
    }, { highWaterMark: 0 }), { status: 400 }) });
    const result = await runProbe(f.options);
    if (bytes === 16384) expect(result.diagnostic).toMatchObject({ response_shape: 'error_object',
      provider_code: 'subscription_sharing_unsupported_capability', provider_param: 'text.format' });
    else expect(result.diagnostic).toEqual({ stage: 'model_request', reason: 'http_error', http_status: 400 });
    expect(cancelled).toBe(bytes === 16385);
    await expectUnknownHttpFailure(f, result, 400);
  });
  it.each(['read', 'cancel'])('retains known HTTP dispatch when diagnostic body %s fails', async failure => {
    let readFailed = false, cancelFailed = false;
    const f = await setup({ response: () => new Response(new ReadableStream({
      pull(controller) {
        if (failure === 'read') { readFailed = true; throw new Error(privateHttpDetail); }
        controller.enqueue(new Uint8Array(16385));
      },
      cancel() { cancelFailed = true; throw new Error(privateHttpDetail); },
    }, { highWaterMark: 0 }), { status: 400 }) });
    const result = await runProbe(f.options);
    expect(result.diagnostic).toEqual({ stage: 'model_request', reason: 'http_error', http_status: 400 });
    expect(failure === 'read' ? readFailed : cancelFailed).toBe(true);
    await expectUnknownHttpFailure(f, result, 400);
  });
  it('uses the same safe diagnostics for a successful HTTP response that is not SSE', async () => {
    const f = await setup({ response: () => Response.json({ error: {
      code: 'subscription_sharing_unsupported_capability', param: 'stream', message: privateHttpDetail,
    } }) });
    const result = await runProbe(f.options);
    expect(result.diagnostic).toEqual({ stage: 'model_request', reason: 'unexpected_content_type', http_status: 200,
      response_shape: 'error_object', provider_code: 'subscription_sharing_unsupported_capability', provider_param: 'stream' });
    await expectUnknownHttpFailure(f, result, 200);
  });
  it('retains known HTTP dispatch when the error response has no body', async () => {
    const f = await setup({ response: () => new Response(null, { status: 400 }) });
    const result = await runProbe(f.options);
    expect(result.diagnostic).toEqual({ stage: 'model_request', reason: 'http_error', http_status: 400 });
    await expectUnknownHttpFailure(f, result, 400);
  });
  it.each([429, 500])('does not retry, change payment path or disclose private error body after HTTP %i', async status => {
    const f = await setup({ response: () => new Response('SYNTHETIC_PRIVATE_ERROR', { status }) });
    const result = await runProbe(f.options);
    expect(result).toMatchObject({ status: 'http_error', http_status: status, usage_status: 'unknown', post_dispatch: 'sent',
      diagnostic: { stage: 'model_request', reason: 'http_error', http_status: status }, model_request_count_not_assumed: true });
    expect(f.calls.filter(c => c.init.method === 'POST')).toHaveLength(1);
    expect(JSON.stringify(result)).not.toContain('PRIVATE');
    expect(await readdir(f.options.evidenceDir)).toContain('unresolved.json');
  });
  it('preserves unknown usage after connection loss and blocks another run before sign-in', async () => {
    const f = await setup({ response: () => { throw new Error('connection lost with private detail'); } });
    expect(await runProbe(f.options)).toMatchObject({ status: 'interrupted', usage_status: 'unknown', post_dispatch: 'possibly_sent',
      diagnostic: { stage: 'model_request', reason: 'transport_error' }, model_request_count_not_assumed: true });
    f.timeline.length = 0;
    expect(await runProbe(f.options)).toMatchObject({ status: 'blocked', reason: 'unresolved_prior_usage' });
    expect(f.timeline).toHaveLength(0);
  });
  it('serializes sign-in and inference and reuses only a nonsecret host UUID', async () => {
    const f = await setup();
    let release; const pause = new Promise(resolve => { release = resolve; });
    const first = runProbe({ ...f.options, confirm: async () => { await pause; return ''; } });
    while (f.timeline.length === 0) await new Promise(resolve => setTimeout(resolve, 1));
    expect(await runProbe(f.options)).toMatchObject({ status: 'blocked', reason: 'another_probe_active' });
    release(); await first;
    const before = await readFile(join(f.options.localDir, 'installation.json'), 'utf8');
    await runProbe({ ...f.options, confirm: async () => '' });
    expect(await readFile(join(f.options.localDir, 'installation.json'), 'utf8')).toBe(before);
  });
  it('waits for stream cancellation, records unknown usage, and never sends a second POST', async () => {
    const controller = new AbortController();
    let cancelled = false;
    const f = await setup({ response: () => new Response(new ReadableStream({
      start() { setTimeout(() => controller.abort(), 5); }, cancel() { cancelled = true; },
    }), { headers: { 'content-type': 'text/event-stream' } }) });
    const result = await runProbe({ ...f.options, signal: controller.signal });
    expect(result).toMatchObject({ status: 'aborted', usage_status: 'unknown', post_dispatch: 'sent',
      diagnostic: { stage: 'model_request', reason: 'cancelled' }, model_request_count_not_assumed: true,
      response_metadata: { http_status: 200, media_type: 'sse', body_present: true, body_read_outcome: 'cancelled' } });
    expect(cancelled).toBe(true);
    expect(f.calls.filter(c => c.init.method === 'POST')).toHaveLength(1);
  });
});

describe('loopback callback lifetime', () => {
  it('listens on 127.0.0.1 before return, rejects bad method/path/state, accepts once and closes', async () => {
    const listener = await createCallbackListener();
    try {
      const url = new URL(listener.redirectUri);
      expect(url.hostname).toBe('127.0.0.1'); expect(url.pathname).toBe('/auth/callback');
      const callback = listener.wait('expected');
      expect((await fetch(url, { method: 'POST' })).status).toBe(400);
      expect((await fetch(new URL('/callback?state=expected', url))).status).toBe(400);
      expect((await fetch(new URL('?state=wrong&code=x', url))).status).toBe(400);
      const good = new URL('?state=expected&code=x&client_id=oaiapp_fixture', url);
      expect((await fetch(good)).status).toBe(200);
      expect(String(await callback)).toBe(String(good));
      expect((await fetch(good)).status).toBe(400);
    } finally { await listener.close(); }
    await expect(fetch(listener.redirectUri)).rejects.toThrow();
  });
  it('returns HTTP 400 for a malformed external URL and keeps the listener usable', async () => {
    const listener = await createCallbackListener();
    try {
      const url = new URL(listener.redirectUri);
      const wire = await new Promise((resolve, reject) => {
        const socket = connect(Number(url.port), '127.0.0.1');
        let text = '';
        socket.setTimeout(500, () => { socket.destroy(); resolve(text); });
        socket.on('data', chunk => { text += chunk.toString(); });
        socket.on('end', () => resolve(text));
        socket.on('error', reject);
        socket.on('connect', () => socket.write(`GET http://[ HTTP/1.1\r\nHost: ${url.host}\r\nConnection: close\r\n\r\n`));
      });
      expect(wire).toMatch(/^HTTP\/1.1 400/);
      expect((await fetch(url)).status).toBe(400);
    } finally { await listener.close(); }
  });
  it('closes a pending callback on cancellation', async () => {
    const controller = new AbortController();
    const listener = await createCallbackListener({ signal: controller.signal });
    const callback = listener.wait('expected'); controller.abort();
    expect(await callback).toBe(null);
    await listener.close();
    await expect(fetch(listener.redirectUri)).rejects.toThrow();
  });
});

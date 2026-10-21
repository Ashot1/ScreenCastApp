import { describe, expect, it } from 'vitest';
import { readAnalysisStream, validateProposal } from './response.mjs';

const proposal = { kind: 'suggestion', message: 'Проверить источники отчёта.', rationale: 'Проверка таблицы закончена.', evidence_ids: ['test-1'] };
const usage = { input_tokens: 100, output_tokens: 20, total_tokens: 120, input_tokens_details: { cached_tokens: 10 } };
const completed = {
  type: 'response.completed',
  response: { id: 'resp_synthetic', model: 'gpt-6-luna', service_tier: 'fast', reasoning: { effort: 'low' }, status: 'completed', usage,
    output: [{ type: 'message', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text: JSON.stringify(proposal) }] }],
  },
};
const encode = value => new TextEncoder().encode(value);
const frame = event => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`;
function stream(text, width = 17) {
  const bytes = encode(text);
  return new ReadableStream({ start(controller) {
    for (let index = 0; index < bytes.length; index += width) controller.enqueue(bytes.slice(index, index + width));
    controller.close();
  } });
}
const read = (body, options = {}) => readAnalysisStream(body, { evidenceIds: ['test-1'], ...options });

describe('proposal validation independent of transport', () => {
  it.each([
    ['payload_type', null],
    ['payload_keys', { ...proposal, PRIVATE_KEY: 'PRIVATE_VALUE' }],
    ['rationale', { ...proposal, rationale: ' ' }],
    ['evidence_ids_type', { ...proposal, evidence_ids: null }],
    ['evidence_ids_value', { ...proposal, evidence_ids: ['PRIVATE_EVIDENCE_ID'] }],
    ['none_message', { ...proposal, kind: 'none' }],
    ['suggestion_shape', { ...proposal, message: '' }],
    ['suggestion_shape', { ...proposal, evidence_ids: [] }],
    ['kind', { ...proposal, kind: 'PRIVATE_KIND' }],
  ])('reports only the existing closed rejection condition %s', (reason, value) => {
    const result = validateProposal(value, ['test-1']);
    expect(result).toMatchObject({ kind: 'invalid', rejection_reason: reason });
    expect(JSON.stringify(result)).not.toContain('PRIVATE');
  });
  it('accepts a proposal only with a known source and a nonempty explanation', () => {
    expect(validateProposal(proposal, ['test-1']).kind).toBe('suggestion');
    expect(validateProposal(proposal, ['other-source']).kind).toBe('invalid');
    expect(validateProposal({ ...proposal, rationale: ' ' }, ['test-1']).kind).toBe('invalid');
    expect(validateProposal({ ...proposal, extra: true }, ['test-1']).kind).toBe('invalid');
  });
  it('keeps silence distinct from a proposal or malformed content', () => {
    expect(validateProposal({ kind: 'none', message: null, rationale: 'Повода нет.', evidence_ids: [] }, []).kind).toBe('none');
    expect(validateProposal({ ...proposal, kind: 'none' }, ['test-1']).kind).toBe('invalid');
    expect(validateProposal(null, []).kind).toBe('invalid');
    expect(validateProposal({ ...proposal, message: '' }, ['test-1']).kind).toBe('invalid');
  });
});

describe('subscription response stream boundary', () => {
  it('reconstructs actual-shaped reasoning and completed message items after completed empty output', async () => {
    const event = structuredClone(completed); event.response.output = [];
    const message = { type: 'message', role: 'assistant', status: 'completed', id: 'PRIVATE_ID',
      content: [{ type: 'output_text', text: JSON.stringify({ kind: 'none', message: null, rationale: 'PRIVATE_TEXT', evidence_ids: [] }) }] };
    const wire = frame({ type: 'response.output_item.done', output_index: 0, item: { type: 'reasoning' } }) +
      frame({ type: 'response.output_item.done', output_index: 1, item: message }) + frame(event);
    const result = await read(stream(wire));
    expect(result).toMatchObject({ status: 'completed', output_source: 'completed_items', proposal: { kind: 'none' }, usage_status: 'reported',
      validation_metadata: { terminal_output_count: 0, output_count: 2, message_count: 1 } });
    expect(result.stream_event_metadata).not.toHaveProperty('candidate');
  });
  it.each([
    ['completed_items_gap', [{ index: 1, item: 'message' }]],
    ['completed_items_duplicate_index', [{ index: 0, item: 'message' }, { index: 0, item: 'message' }]],
    ['completed_items_invalid_index', [{ index: 8, item: 'message' }]],
    ['message_status', [{ index: 0, item: 'unfinished_message' }]],
    ['output_item_type', [{ index: 0, item: 'tool' }]],
    ['completed_items_unfinished', [{ index: 0, item: 'unfinished_reasoning' }]],
  ])('rejects completed-item sequence %s while retaining terminal usage', async (reason, entries) => {
    const variants = { message: completed.response.output[0], unfinished_message: { ...completed.response.output[0], status: 'in_progress' },
      tool: { type: 'PRIVATE_TOOL' }, unfinished_reasoning: { type: 'reasoning', status: 'in_progress' } };
    const event = structuredClone(completed); event.response.output = [];
    const wire = entries.map(entry => frame({ type: 'response.output_item.done', output_index: entry.index, item: variants[entry.item] })).join('') + frame(event);
    const result = await read(stream(wire));
    expect(result).toMatchObject({ status: 'completed', output_source: 'unavailable', proposal: { kind: 'invalid' }, usage_status: 'reported',
      validation_metadata: { proposal_rejection: reason } });
    expect(JSON.stringify(result.validation_metadata) + JSON.stringify(result.stream_event_metadata)).not.toContain('PRIVATE');
  });
  it('rejects an added item that never receives a done event', async () => {
    const event = structuredClone(completed); event.response.output = [];
    const result = await read(stream(frame({ type: 'response.output_item.added', output_index: 0, item: { type: 'message', status: 'in_progress' } }) + frame(event)));
    expect(result).toMatchObject({ output_source: 'unavailable', usage_status: 'reported', validation_metadata: { proposal_rejection: 'completed_items_unfinished' } });
  });
  it('uses authoritative nonempty terminal output even if an added item has no done event', async () => {
    const result = await read(stream(frame({ type: 'response.output_item.added', output_index: 0,
      item: { type: 'message', status: 'in_progress' } }) + frame(completed)));
    expect(result).toMatchObject({ output_source: 'terminal', proposal: { kind: 'suggestion' }, usage_status: 'reported' });
  });
  it.each([false, true])('keeps terminal output authoritative and rejects observed conflict=%j', async conflict => {
    const item = structuredClone(completed.response.output[0]); item.id = 'PRIVATE_DIFFERENT_ID';
    if (conflict) item.content[0].text = JSON.stringify({ ...proposal, rationale: 'PRIVATE_DIFFERENT_TEXT' });
    const result = await read(stream(frame({ type: 'response.output_item.done', output_index: 0, item }) + frame(completed)));
    expect(result).toMatchObject({ status: 'completed', output_source: conflict ? 'unavailable' : 'terminal',
      proposal: { kind: conflict ? 'invalid' : 'suggestion' }, usage_status: 'reported' });
    if (conflict) expect(result.validation_metadata.proposal_rejection).toBe('completed_item_conflict');
  });
  it.each([65536, 65537])('bounds retained completed-item JSON at %i bytes without truncating it', async bytes => {
    const item = structuredClone(completed.response.output[0]);
    const payload = { kind: 'none', message: null, rationale: '', evidence_ids: [] };
    item.content[0].text = JSON.stringify(payload);
    payload.rationale = 'x'.repeat(bytes - Buffer.byteLength(JSON.stringify(item)));
    item.content[0].text = JSON.stringify(payload);
    expect(Buffer.byteLength(JSON.stringify(item))).toBe(bytes);
    const event = structuredClone(completed); event.response.output = [];
    const result = await read(stream(frame({ type: 'response.output_item.done', output_index: 0, item }) + frame(event)));
    expect(result).toMatchObject({ usage_status: 'reported', output_source: bytes === 65536 ? 'completed_items' : 'unavailable',
      proposal: { kind: bytes === 65536 ? 'none' : 'invalid' } });
    if (bytes > 65536) expect(result.validation_metadata.proposal_rejection).toBe('completed_items_overflow');
  });
  it('never accepts done items without response.completed', async () => {
    const result = await read(stream(frame({ type: 'response.output_item.done', output_index: 0, item: completed.response.output[0] })));
    expect(result).toMatchObject({ status: 'incomplete', output_source: 'unavailable', usage_status: 'unknown' });
    expect(result).not.toHaveProperty('proposal');
  });
  it('discards retained item text on cancellation before completion', async () => {
    const controller = new AbortController();
    const item = structuredClone(completed.response.output[0]); item.id = 'PRIVATE_ID'; item.content[0].text = 'PRIVATE_TEXT';
    const body = new ReadableStream({ start(source) {
      source.enqueue(encode(frame({ type: 'response.output_item.done', output_index: 0, item })));
      setTimeout(() => controller.abort(), 5);
    } });
    const result = await read(body, { signal: controller.signal });
    expect(result).toMatchObject({ status: 'aborted', output_source: 'unavailable', usage_status: 'unknown' });
    expect(result).not.toHaveProperty('proposal');
    expect(JSON.stringify(result)).not.toContain('PRIVATE');
  });
  it.each([true, false])('does not reconstruct missing/non-array terminal output (missing=%j)', async missing => {
    const event = structuredClone(completed);
    if (missing) delete event.response.output; else event.response.output = null;
    const result = await read(stream(frame({ type: 'response.output_item.done', output_index: 0, item: completed.response.output[0] }) + frame(event)));
    expect(result).toMatchObject({ output_source: 'unavailable', proposal: { kind: 'invalid' }, usage_status: 'reported',
      validation_metadata: { proposal_rejection: missing ? 'output_missing' : 'output_type' } });
  });
  it('keeps delta text as bounded counters only, without a second acceptance path', async () => {
    const event = structuredClone(completed); event.response.output = [];
    const wire = frame({ type: 'response.output_text.delta', output_index: 0, content_index: 0, delta: JSON.stringify(proposal) }) + frame(event);
    const result = await read(stream(wire));
    expect(result).toMatchObject({ output_source: 'unavailable', proposal: { kind: 'invalid' },
      stream_event_metadata: { event_counts: { 'response.output_text.delta': 1 } } });
    expect(result.stream_event_metadata).not.toHaveProperty('candidate');
  });
  it.each([-1, 21, '3', 1.5])('omits an invalid reasoning-token detail %j without losing usage', async reasoning_tokens => {
    const event = structuredClone(completed); event.response.usage.output_tokens_details = { reasoning_tokens };
    const result = await read(stream(frame(event)));
    expect(result.usage_status).toBe('reported');
    expect(result.usage).not.toHaveProperty('output_tokens_details');
  });
  it('distinguishes a missing message status using fixed type flags and bounded counts', async () => {
    const event = structuredClone(completed); delete event.response.output[0].status;
    const result = await read(stream(frame(event)));
    expect(result.validation_metadata).toMatchObject({ proposal_rejection: 'message_status', output_count: 1, message_count: 1,
      message_fields: { role: 'string', status: 'missing', content: 'array' } });
  });
  it('reports fixed payload key-presence flags without an unexpected key name', async () => {
    const event = structuredClone(completed);
    event.response.output[0].content[0].text = JSON.stringify({ kind: 'none', message: null, evidence_ids: [], PRIVATE_KEY: 'PRIVATE_VALUE' });
    const result = await read(stream(frame(event)));
    expect(result.validation_metadata).toMatchObject({ proposal_rejection: 'payload_keys',
      proposal_keys: { kind: true, message: true, rationale: false, evidence_ids: true, unexpected: true } });
    expect(JSON.stringify(result.validation_metadata)).not.toContain('PRIVATE');
  });
  it.each([
    ['output_missing', response => { delete response.output; }],
    ['output_type', response => { response.output = {}; }],
    ['output_item_type', response => { response.output = [{ type: 'PRIVATE_TOOL' }]; }],
    ['message_count', response => { response.output = []; }],
    ['message_role', response => { response.output[0].role = 'PRIVATE_ROLE'; }],
    ['message_status', response => { response.output[0].status = 'PRIVATE_STATUS'; }],
    ['content_type', response => { response.output[0].content = {}; }],
    ['content_count', response => { response.output[0].content = []; }],
    ['content_item_type', response => { response.output[0].content = [{ type: 'PRIVATE_TYPE' }]; }],
    ['output_text_type', response => { response.output[0].content[0].text = {}; }],
    ['json_parse', response => { response.output[0].content[0].text = 'PRIVATE_INVALID_JSON'; }],
    ['payload_keys', response => { response.output[0].content[0].text = JSON.stringify({ ...proposal, PRIVATE_KEY: 'PRIVATE_VALUE' }); }],
  ])('preserves usage and a safe exact proposal rejection %s', async (reason, mutate) => {
    const event = structuredClone(completed); mutate(event.response);
    const result = await read(stream(frame(event)));
    expect(result).toMatchObject({ status: 'completed', usage_status: 'reported', usage,
      proposal: { kind: 'invalid' }, validation_metadata: { proposal_rejection: reason } });
    expect(JSON.stringify(result.validation_metadata)).not.toContain('PRIVATE');
  });
  it.each([
    ['terminal_event', () => stream(frame(completed))],
    ['empty', () => stream('')],
    ['eof', () => stream('data: [DONE]\n\n')],
    ['invalid_utf8', () => new ReadableStream({ start(controller) { controller.enqueue(new Uint8Array([255])); controller.close(); } })],
    ['invalid_json', () => stream('data: broken\n\n')],
    ['invalid_event', () => stream('data: {}\n\n')],
    ['read_error', () => new ReadableStream({ start(controller) { controller.error(new Error('PRIVATE_ERROR')); } })],
  ])('preserves a closed body reading outcome %s without raw data', async (outcome, body) => {
    const result = await read(body());
    expect(result.read_metadata).toMatchObject({ body_read_outcome: outcome, bytes_observed: expect.any(Number), byte_limit: 2 * 1024 * 1024 });
    expect(JSON.stringify(result.read_metadata)).not.toContain('PRIVATE');
  });
  it('waits for response.completed across UTF-8 and CRLF chunk boundaries', async () => {
    const result = await read(stream(frame(completed).replaceAll('\n', '\r\n'), 1));
    expect(result).toMatchObject({ status: 'completed', returned_configuration: { model: 'gpt-6-luna', service_tier: 'fast', reasoning_effort: 'low' }, proposal: { kind: 'suggestion', payload: proposal }, usage, usage_status: 'reported' });
  });
  it('supports SSE comments, bare CR and multiline data without treating them as model instructions', async () => {
    const json = JSON.stringify(completed);
    const wire = `: keepalive\r\revent: response.completed\rdata: ${json.slice(0, 1)}\rdata: ${json.slice(1)}\r\r`;
    expect((await read(stream(wire, 2))).status).toBe('completed');
  });
  it('does not accept a delta or EOF as a final answer, even if the text is valid JSON', async () => {
    const delta = { type: 'response.output_text.delta', delta: JSON.stringify(proposal) };
    for (const wire of [frame(delta), frame(completed).trimEnd(), 'data: [DONE]\n\n']) {
      expect(await read(stream(wire))).toMatchObject({ status: 'incomplete', usage: null, usage_status: 'unknown' });
    }
  });
  it('preserves reported usage when the completed answer has invalid content', async () => {
    const event = structuredClone(completed);
    event.response.output[0].content[0].text = '{';
    expect(await read(stream(frame(event)))).toMatchObject({ status: 'completed', proposal: { kind: 'invalid' }, usage_status: 'reported', usage });
  });
  it('preserves usage but rejects an invalid returned reasoning-effort value', async () => {
    for (const effort of ['invented', 1]) {
      const event = structuredClone(completed);
      event.response.reasoning.effort = effort;
      expect(await read(stream(frame(event)))).toMatchObject({ status: 'invalid', usage_status: 'reported', usage });
    }
  });
  it('separates refusal, failed quota and a genuinely incomplete response', async () => {
    const refusal = structuredClone(completed);
    refusal.response.output[0].content = [{ type: 'refusal', refusal: 'Synthetic safety refusal.' }];
    expect((await read(stream(frame(refusal)))).proposal.kind).toBe('refusal');
    const failed = { type: 'response.failed', response: { status: 'failed', model: 'gpt-6-luna', service_tier: 'priority',
      reasoning: { effort: 'high' }, error: { code: 'subscription_sharing_usage_limit_exceeded' } } };
    expect(await read(stream(frame(failed)))).toMatchObject({ status: 'failed', failure_code: 'subscription_sharing_usage_limit_exceeded', usage: null, usage_status: 'unknown',
      returned_configuration: { model: 'gpt-6-luna', service_tier: 'priority', reasoning_effort: 'high' } });
    const incomplete = { type: 'response.incomplete', response: { status: 'incomplete', model: 'gpt-6-luna', service_tier: 'default', reasoning: { effort: 'low' }, usage } };
    expect(await read(stream(frame(incomplete)))).toMatchObject({ status: 'incomplete', usage_status: 'reported', usage,
      returned_configuration: { model: 'gpt-6-luna', service_tier: 'default', reasoning_effort: 'low' } });
  });
  it('keeps usage-service failure distinct without persisting raw provider errors', async () => {
    const error = { type: 'error', code: 'subscription_sharing_usage_unavailable', message: 'SYNTHETIC_PRIVATE_DETAIL' };
    const result = await read(stream(frame(error)));
    expect(result).toMatchObject({ status: 'failed', failure_code: 'subscription_sharing_usage_unavailable', usage_status: 'unknown' });
    expect(JSON.stringify(result)).not.toContain('PRIVATE');
  });
  it('preserves the known unsupported SIWC capability code without retrying or leaking its message', async () => {
    const failed = { type: 'response.failed', response: { status: 'failed', error: {
      code: 'subscription_sharing_unsupported_capability', message: 'SYNTHETIC_PRIVATE_DETAIL',
    } } };
    const result = await read(stream(frame(failed)));
    expect(result).toMatchObject({ status: 'failed', failure_code: 'subscription_sharing_unsupported_capability', usage_status: 'unknown' });
    expect(JSON.stringify(result)).not.toContain('PRIVATE');
  });
  it('does not invent usage for missing, inconsistent or invalid counters', async () => {
    for (const value of [null, { input_tokens: 1, output_tokens: -1 }, { input_tokens: 1, output_tokens: 2, total_tokens: 7 }]) {
      const event = structuredClone(completed);
      event.response.usage = value;
      expect(await read(stream(frame(event)))).toMatchObject({ status: 'completed', usage: null, usage_status: 'unknown' });
    }
  });
  it('rejects malformed events, mismatched terminal status and unexpected tool output', async () => {
    const tool = structuredClone(completed);
    tool.response.output = [{ type: 'function_call', name: 'unauthorized_action' }];
    expect((await read(stream(frame(tool)))).proposal.kind).toBe('invalid');
    const mismatch = structuredClone(completed);
    mismatch.response.status = 'in_progress';
    expect((await read(stream(frame(mismatch)))).status).toBe('invalid');
    expect((await read(stream('event: response.completed\ndata: broken\n\n'))).status).toBe('invalid');
  });
  it('bounds bytes, frame size and event count and keeps missing usage unknown', async () => {
    expect((await read(stream(frame(completed)), { maximumBytes: 20 })).status).toBe('limit_exceeded');
    expect((await read(stream(frame(completed)), { maximumFrameBytes: 20 })).status).toBe('limit_exceeded');
    const created = { type: 'response.created', response: { status: 'in_progress' } };
    expect(await read(stream(frame(created) + frame(created)), { maximumEvents: 1 })).toMatchObject({ status: 'limit_exceeded', usage: null });
  });
  it('records local abort only after reader cancellation settles and does not claim remote stop', async () => {
    const controller = new AbortController();
    let confirmCancellation;
    let cancellationStarted;
    const started = new Promise(resolve => { cancellationStarted = resolve; });
    const body = new ReadableStream({
      cancel() { cancellationStarted(); return new Promise(resolve => { confirmCancellation = resolve; }); },
    });
    let finished = false;
    const result = read(body, { signal: controller.signal }).then(value => { finished = true; return value; });
    controller.abort();
    await started;
    await Promise.resolve();
    expect(finished).toBe(false);
    confirmCancellation();
    expect(await result).toMatchObject({ status: 'aborted', usage: null, usage_status: 'unknown' });
    expect(body.locked).toBe(false);
  });
  it('keeps loss of the connection separate from a successful terminal response', async () => {
    const body = new ReadableStream({ start(controller) { controller.error(new TypeError('synthetic connection loss')); } });
    expect(await read(body)).toMatchObject({ status: 'interrupted', usage: null, usage_status: 'unknown' });
    expect(body.locked).toBe(false);
  });
});

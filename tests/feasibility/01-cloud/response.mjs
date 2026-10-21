// Synthetic feasibility boundary checks; no network or credential access.
import { isDeepStrictEqual } from 'node:util';
const invalidProposal = (reason, metadata = {}) => ({ kind: 'invalid', rejection_reason: reason, validation_metadata: metadata });
const boundedCount = value => Math.min(value, 1000);
function fieldType(object, key) {
  if (!object || !Object.hasOwn(object, key)) return 'missing';
  const value = object[key];
  return value === null ? 'null' : Array.isArray(value) ? 'array' : typeof value;
}
function comparisonState(object, key, requested) {
  const type = fieldType(object, key);
  return ['missing', 'null'].includes(type) ? type : type === 'string' && object[key] === requested ? 'matching' : 'different';
}
function tierState(response) {
  const type = fieldType(response, 'service_tier');
  if (['missing', 'null'].includes(type)) return type;
  if (type !== 'string') return 'other_type';
  return ['fast', 'priority', 'auto', 'default', 'flex', 'standard'].includes(response.service_tier) ? response.service_tier : 'other_string';
}

export function validateProposal(payload, evidenceIds) {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return invalidProposal('payload_type');
  const keys = Object.keys(payload);
  const metadata = { proposal_keys: Object.fromEntries(['kind', 'message', 'rationale', 'evidence_ids'].map(key => [key, Object.hasOwn(payload, key)])) };
  metadata.proposal_keys.unexpected = keys.some(key => !['kind', 'message', 'rationale', 'evidence_ids'].includes(key));
  if (keys.sort().join(',') !== 'evidence_ids,kind,message,rationale') return invalidProposal('payload_keys', metadata);
  if (typeof payload.rationale !== 'string' || !payload.rationale.trim()) return invalidProposal('rationale', metadata);
  if (!Array.isArray(payload.evidence_ids)) return invalidProposal('evidence_ids_type', metadata);
  metadata.evidence_id_count = boundedCount(payload.evidence_ids.length);
  if (payload.evidence_ids.some(id => typeof id !== 'string' || !evidenceIds.includes(id))) return invalidProposal('evidence_ids_value', metadata);
  if (payload.kind === 'none') return payload.message === null ? { kind: 'none', payload, validation_metadata: metadata } : invalidProposal('none_message', metadata);
  if (payload.kind === 'suggestion' && typeof payload.message === 'string' && payload.message.trim() && payload.evidence_ids.length) {
    return { kind: 'suggestion', payload, validation_metadata: metadata };
  }
  if (payload.kind === 'suggestion') {
    metadata.suggestion_message_nonempty = typeof payload.message === 'string' && Boolean(payload.message.trim());
    return invalidProposal('suggestion_shape', metadata);
  }
  return invalidProposal('kind', metadata);
}

function proposalFromResponse(response, evidenceIds) {
  const metadata = {};
  if (!Object.hasOwn(response, 'output')) return invalidProposal('output_missing', metadata);
  if (!Array.isArray(response.output)) return invalidProposal('output_type', metadata);
  metadata.output_count = boundedCount(response.output.length);
  if (response.output.some(item => !item || !['reasoning', 'message'].includes(item.type))) return invalidProposal('output_item_type', metadata);
  const messages = response.output.filter(item => item.type === 'message');
  metadata.message_count = boundedCount(messages.length);
  metadata.reasoning_count = boundedCount(response.output.length - messages.length);
  if (messages.length !== 1) return invalidProposal('message_count', metadata);
  metadata.message_fields = Object.fromEntries(['role', 'status', 'content'].map(key => [key, fieldType(messages[0], key)]));
  if (messages[0].role !== 'assistant') return invalidProposal('message_role', metadata);
  if (messages[0].status !== 'completed') return invalidProposal('message_status', metadata);
  const content = messages[0].content;
  if (!Array.isArray(content)) return invalidProposal('content_type', metadata);
  metadata.content_count = boundedCount(content.length);
  if (content.length !== 1) return invalidProposal('content_count', metadata);
  if (content[0]?.type === 'refusal' && typeof content[0].refusal === 'string') return { kind: 'refusal', validation_metadata: metadata };
  if (content[0]?.type !== 'output_text') return invalidProposal('content_item_type', metadata);
  if (typeof content[0].text !== 'string') return invalidProposal('output_text_type', metadata);
  let payload;
  try { payload = JSON.parse(content[0].text); } catch { return invalidProposal('json_parse', metadata); }
  const result = validateProposal(payload, evidenceIds);
  return { ...result, validation_metadata: { ...metadata, ...result.validation_metadata } };
}

function canonicalItem(item) {
  if (!item || typeof item !== 'object' || Array.isArray(item)) return null;
  const fields = item.type === 'message' ? ['type', 'role', 'status', 'content'] : ['type'];
  const value = Object.fromEntries(fields.filter(key => Object.hasOwn(item, key)).map(key => [key, item[key]]));
  if (Array.isArray(value.content)) value.content = value.content.map(part => {
    if (!part || typeof part !== 'object' || Array.isArray(part)) return null;
    const fields = part.type === 'output_text' ? ['type', 'text'] : part.type === 'refusal' ? ['type', 'refusal'] : ['type'];
    return Object.fromEntries(fields.filter(key => Object.hasOwn(part, key)).map(key => [key, part[key]]));
  });
  return value;
}

function createStreamDiagnostics(evidenceIds) {
  const maximumItemBytes = 64 * 1024, maximumParts = 8;
  const eventCounts = Object.fromEntries(['response.created', 'response.in_progress', 'response.output_item.added',
    'response.output_item.done', 'response.content_part.added', 'response.content_part.done', 'response.output_text.delta',
    'response.output_text.done', 'response.completed', 'response.failed', 'response.incomplete', 'error', 'other'].map(type => [type, 0]));
  const parts = new Map(), itemsDone = [], completedItems = new Map(), pendingItems = new Set();
  let completedItemBytes = 0, completedItemError, deltaBytes = 0, invalidDeltas = 0;
  let invalidIndexes = 0, partsTruncated = false, itemsTruncated = false;
  const validIndex = value => Number.isInteger(value) && value >= 0 && value < maximumParts;
  const textBytes = value => typeof value === 'string' ? Buffer.byteLength(value) : 0;
  const boundedBytes = value => Math.min(value, maximumItemBytes + 1);
  const rejectItems = reason => {
    completedItemError ??= reason;
    completedItems.clear();
    pendingItems.clear();
  };
  const closedValue = (object, key, allowed) => {
    const type = fieldType(object, key);
    return type === 'string' ? allowed.includes(object[key]) ? object[key] : 'other_string' : type;
  };
  return {
    observe(event) {
      const type = Object.hasOwn(eventCounts, event.type) ? event.type : 'other';
      eventCounts[type] = boundedCount(eventCounts[type] + 1);
      if (['response.output_item.added', 'response.output_item.done'].includes(event.type) && !completedItemError) {
        const index = event.output_index;
        if (!validIndex(index)) rejectItems('completed_items_invalid_index');
        else if (event.type === 'response.output_item.added') {
          if (pendingItems.has(index) || completedItems.has(index)) rejectItems('completed_items_duplicate_index');
          else pendingItems.add(index);
        } else if (completedItems.has(index)) rejectItems('completed_items_duplicate_index');
        else if (event.item?.type === 'reasoning' && Object.hasOwn(event.item, 'status') && event.item.status !== 'completed') rejectItems('completed_items_unfinished');
        else {
          const item = canonicalItem(event.item);
          // Bound the entire canonical JSON, including content; never retain a truncated completed item.
          completedItemBytes = boundedBytes(completedItemBytes + Buffer.byteLength(JSON.stringify(item)));
          if (completedItemBytes > maximumItemBytes) rejectItems('completed_items_overflow');
          else { completedItems.set(index, item); pendingItems.delete(index); }
        }
      }
      if (event.type === 'response.output_item.done') {
        if (itemsDone.length === maximumParts) itemsTruncated = true;
        else {
          const indexValid = validIndex(event.output_index), content = event.item?.content;
          const item = { index_valid: indexValid, ...(indexValid ? { output_index: event.output_index } : {}),
            item_type: closedValue(event.item, 'type', ['message', 'reasoning']),
            item_status: closedValue(event.item, 'status', ['completed', 'in_progress', 'incomplete', 'failed', 'queued', 'cancelled']),
            content_type: fieldType(event.item, 'content') };
          if (Array.isArray(content)) {
            item.content_count = boundedCount(content.length);
            item.content_types = { output_text: 0, refusal: 0, other: 0 };
            item.text_bytes = 0;
            for (const part of content) {
              const kind = ['output_text', 'refusal'].includes(part?.type) ? part.type : 'other';
              item.content_types[kind] = boundedCount(item.content_types[kind] + 1);
              if (kind === 'output_text') item.text_bytes = boundedBytes(item.text_bytes + textBytes(part.text));
            }
          }
          itemsDone.push(item);
        }
      }
      if (!['response.output_text.delta', 'response.output_text.done'].includes(event.type)) return;
      if (!validIndex(event.output_index) || !validIndex(event.content_index)) {
        invalidIndexes = boundedCount(invalidIndexes + 1); return;
      }
      const key = `${event.output_index}:${event.content_index}`;
      if (!parts.has(key)) {
        if (parts.size === maximumParts) { partsTruncated = true; return; }
        parts.set(key, { output_index: event.output_index, content_index: event.content_index, delta_count: 0, text_bytes: 0, done_count: 0 });
      }
      const part = parts.get(key);
      if (event.type === 'response.output_text.done') {
        part.done_count = boundedCount(part.done_count + 1);
        part.done_text_type = fieldType(event, 'text');
        part.done_text_bytes = boundedBytes(textBytes(event.text));
        return;
      }
      part.delta_count = boundedCount(part.delta_count + 1);
      if (typeof event.delta !== 'string') { invalidDeltas = boundedCount(invalidDeltas + 1); return; }
      const length = textBytes(event.delta);
      part.text_bytes = boundedBytes(part.text_bytes + length);
      deltaBytes = boundedBytes(deltaBytes + length);
    },
    proposal(response) {
      const reject = reason => ({ source: 'unavailable', result: invalidProposal(reason) });
      if (!Array.isArray(response.output)) return { source: 'unavailable', result: proposalFromResponse(response, evidenceIds) };
      if (completedItemError) return reject(completedItemError);
      let output = response.output, source = 'terminal';
      if (output.length) {
        // Compare only validator-relevant fields; provider IDs and unrelated metadata do not define content.
        for (const [index, item] of completedItems) {
          if (index >= output.length || !isDeepStrictEqual(canonicalItem(output[index]), item)) return reject('completed_item_conflict');
        }
      } else {
        if (pendingItems.size) return reject('completed_items_unfinished');
        source = 'completed_items'; output = [];
        for (let index = 0; index < completedItems.size; index++) {
          if (!completedItems.has(index)) return reject('completed_items_gap');
          output.push(completedItems.get(index));
        }
      }
      const result = proposalFromResponse({ ...response, output }, evidenceIds);
      return { source: result.kind === 'invalid' ? 'unavailable' : source, result };
    },
    finish() {
      const metadata = { event_counts: eventCounts, parts: [...parts.values()], output_items_done: itemsDone,
        invalid_index_count: invalidIndexes, parts_truncated: partsTruncated, output_items_truncated: itemsTruncated,
        invalid_delta_count: invalidDeltas, delta_bytes_observed: deltaBytes, completed_item_count: completedItems.size,
        completed_item_bytes_observed: completedItemBytes, completed_item_byte_limit: maximumItemBytes,
        ...(completedItemError ? { completed_item_rejection: completedItemError } : {}), part_limit: maximumParts, index_limit: maximumParts - 1 };
      completedItems.clear(); pendingItems.clear();
      return metadata;
    },
  };
}

function reportedUsage(value) {
  const counter = number => Number.isSafeInteger(number) && number >= 0;
  if (!value || !counter(value.input_tokens) || !counter(value.output_tokens) ||
      (value.total_tokens !== undefined && value.total_tokens !== value.input_tokens + value.output_tokens)) return null;
  const usage = { input_tokens: value.input_tokens, output_tokens: value.output_tokens };
  if (value.total_tokens !== undefined) usage.total_tokens = value.total_tokens;
  const details = value.input_tokens_details;
  if (details) {
    usage.input_tokens_details = {};
    for (const key of ['cached_tokens', 'cache_write_tokens']) {
      if (details[key] !== undefined) {
        if (!counter(details[key]) || details[key] > value.input_tokens) return null;
        usage.input_tokens_details[key] = details[key];
      }
    }
    if ((details.cached_tokens ?? 0) + (details.cache_write_tokens ?? 0) > value.input_tokens) return null;
  }
  const reasoningTokens = value.output_tokens_details?.reasoning_tokens;
  if (counter(reasoningTokens) && reasoningTokens <= value.output_tokens) usage.output_tokens_details = { reasoning_tokens: reasoningTokens };
  return usage;
}

export function failureCode(code) {
  return ['subscription_sharing_usage_limit_exceeded', 'subscription_sharing_usage_unavailable',
    'subscription_sharing_unsupported_capability'].includes(code) ? code : 'provider_failure';
}

export async function readAnalysisStream(body, {
  evidenceIds, signal, requestedModel, requestedReasoningEffort,
  maximumBytes = 2 * 1024 * 1024, maximumFrameBytes = 512 * 1024, maximumEvents = 4096,
}) {
  let bytes = 0;
  const streamDiagnostics = createStreamDiagnostics(evidenceIds);
  const withReadMetadata = (result, outcome) => ({ ...result, output_source: result.output_source ?? 'unavailable', stream_event_metadata: streamDiagnostics.finish(), read_metadata: {
    body_read_outcome: outcome, bytes_observed: bytes, byte_limit: maximumBytes,
  } });
  const unknown = (status, outcome) => withReadMetadata({ status, usage: null, usage_status: 'unknown' }, outcome);
  const reasoningEfforts = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];
  if (!body || typeof body.getReader !== 'function') return unknown('invalid', 'empty');
  let reader;
  try { reader = body.getReader(); } catch { return unknown('interrupted', 'read_error'); }
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let events = 0, frameBytes = 0;
  let line = '', data = [], eventName = '', previousCR = false, terminal, cancellation;
  const fail = (status, readOutcome = status === 'limit_exceeded' ? 'limit_exceeded' : 'invalid_event') => {
    throw Object.assign(new Error('invalid_stream'), { streamStatus: status, readOutcome });
  };
  const decode = (value, options) => {
    try { return decoder.decode(value, options); } catch { fail('invalid', 'invalid_utf8'); }
  };
  const cancel = () => {
    // Closing a local reader is not proof that inference stopped; absent terminal usage remains unknown.
    cancellation ??= reader.cancel().catch(() => undefined);
    return cancellation;
  };
  const dispatch = () => {
    const text = data.join('\n');
    data = [];
    frameBytes = 0;
    const declared = eventName;
    eventName = '';
    if (!text || text === '[DONE]') return;
    if (++events > maximumEvents) fail('limit_exceeded');
    let event;
    try { event = JSON.parse(text); } catch { fail('invalid', 'invalid_json'); }
    if (typeof event?.type !== 'string' || (declared && declared !== event.type)) fail('invalid');
    streamDiagnostics.observe(event);
    const status = { 'response.completed': 'completed', 'response.failed': 'failed', 'response.incomplete': 'incomplete' }[event.type];
    if (status) {
      if (!event.response || event.response.status !== status) fail('invalid');
      const usage = reportedUsage(event.response.usage);
      terminal = { status, usage, usage_status: usage ? 'reported' : 'unknown' };
      if (status === 'failed') terminal.failure_code = failureCode(event.response.error?.code);
      const reasoning = event.response.reasoning;
      const effort = reasoning && typeof reasoning === 'object' && !Array.isArray(reasoning) ? reasoning.effort : null;
      terminal.returned_configuration = {
        model: typeof event.response.model === 'string' ? event.response.model : null,
        service_tier: typeof event.response.service_tier === 'string' ? event.response.service_tier : null,
        reasoning_effort: typeof effort === 'string' ? effort : null,
      };
      // Preserve closed boundary facts before the probe removes unrecognized values and model text.
      terminal.validation_metadata = {
        terminal_fields: Object.fromEntries(['output', 'model', 'service_tier', 'reasoning', 'status'].map(key => [key, fieldType(event.response, key)])),
        configuration: { model: comparisonState(event.response, 'model', requestedModel), service_tier: tierState(event.response),
          reasoning_effort: comparisonState(reasoning, 'effort', requestedReasoningEffort) },
      };
      if (Array.isArray(event.response.output)) terminal.validation_metadata.terminal_output_count = boundedCount(event.response.output.length);
      if (status === 'completed') {
        const selected = streamDiagnostics.proposal(event.response);
        terminal.output_source = selected.source;
        const { rejection_reason: rejection, validation_metadata: proposalMetadata, ...proposal } = selected.result;
        terminal.proposal = proposal;
        Object.assign(terminal.validation_metadata, proposalMetadata, rejection ? { proposal_rejection: rejection } : {});
        const invalidEffort = reasoning !== undefined && reasoning !== null &&
          (!reasoning || typeof reasoning !== 'object' || Array.isArray(reasoning) ||
            (effort !== undefined && effort !== null && !reasoningEfforts.includes(effort)));
        // These response fields report configuration metadata; they do not expose internal reasoning.
        if (invalidEffort) terminal.status = 'invalid';
      }
    } else if (event.type === 'error') terminal = { status: 'failed', usage: null, usage_status: 'unknown', failure_code: failureCode(event.code) };
  };
  const finishLine = () => {
    if (!line) dispatch();
    else {
      const colon = line.indexOf(':');
      const field = colon === -1 ? line : line.slice(0, colon);
      let value = colon === -1 ? '' : line.slice(colon + 1);
      if (value.startsWith(' ')) value = value.slice(1);
      if (field === 'data') data.push(value);
      else if (field === 'event') eventName = value;
    }
    line = '';
  };
  const accept = text => {
    for (const character of text) {
      if (terminal) break;
      if (character === '\n' && previousCR) { previousCR = false; continue; }
      previousCR = character === '\r';
      frameBytes += Buffer.byteLength(character);
      if (frameBytes > maximumFrameBytes) fail('limit_exceeded');
      if (character === '\r' || character === '\n') finishLine();
      else line += character;
    }
  };
  signal?.addEventListener('abort', cancel, { once: true });
  try {
    if (signal?.aborted) { await cancel(); return unknown('aborted', 'cancelled'); }
    while (!terminal) {
      const { done, value } = await reader.read();
      if (!done) bytes = Math.min(maximumBytes + 1, bytes + value.byteLength);
      if (signal?.aborted) return unknown('aborted', 'cancelled');
      if (done) { accept(decode()); break; }
      if (bytes > maximumBytes) fail('limit_exceeded');
      accept(decode(value, { stream: true }));
    }
    return terminal ? withReadMetadata(terminal, 'terminal_event') : unknown('incomplete', bytes ? 'eof' : 'empty');
  } catch (error) {
    return unknown(signal?.aborted ? 'aborted' : error.streamStatus ?? 'interrupted', signal?.aborted ? 'cancelled' : error.readOutcome ?? 'read_error');
  } finally {
    signal?.removeEventListener('abort', cancel);
    await cancel();
    reader.releaseLock();
  }
}

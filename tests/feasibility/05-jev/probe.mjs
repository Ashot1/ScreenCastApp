// Test-only tooling; no application module imports this file.
import { readFileSync } from 'node:fs';
import { createHash, randomUUID } from 'node:crypto';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { initializeLedger, reserve, settle, budgetStatus, parseUsd, warningForExposure, validPurpose } from './budget.mjs';

const model = 'jev-1.14.0';
const endpoint = 'https://api.typesafe.ai/v1/systemone';
const sharedLedger = fileURLToPath(new URL('../../../docs/features/assistant-behavior/evidence/05-jev/jev-tests.sqlite3', import.meta.url));
const settingsURL = new URL('./live-settings.json', import.meta.url);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const manifest = JSON.parse(readFileSync(new URL('./fixtures/manifest.json', import.meta.url), 'utf8'));
const cases = Object.fromEntries(['P', 'A', 'M'].flatMap((prefix, index) =>
  ['en', 'ru', 'image'].map(mode => [`${prefix}-${mode}`, { file: ['present', 'absent', 'ambiguous'][index], mode }])));
cases['R-en'] = { file: 'present-half', mode: 'en' };
cases['C-en'] = { file: 'present-crop', mode: 'en' };

function fixture(name) {
  const info = manifest.fixtures.find(item => item.file === `${name}.png`);
  const bytes = readFileSync(new URL(`./fixtures/${name}.png`, import.meta.url));
  if (!info || hash(bytes) !== info.sha256 || bytes.length !== info.bytes || bytes.length > 16 * 1024 * 1024 ||
      !info.dimensions.every(value => Number.isInteger(value) && value >= 280 && value <= 1568)) {
    throw new Error('changed_or_invalid_fixture');
  }
  return { info, bytes };
}

export function buildRequest(caseId) {
  const selection = Object.hasOwn(cases, caseId) ? cases[caseId] : null;
  if (!selection) throw new Error('unknown_case');
  const scene = fixture(selection.file);
  const files = [{ name: 'scene', part: 'scene-photo', ...scene }];
  if (selection.mode === 'image') files.push({ name: 'reference', part: 'reference-photo', ...fixture('exemplar') });
  const state = {};
  const assets = {};
  for (const file of files) {
    const reference = randomUUID();
    state[file.name] = reference;
    assets[reference] = { type: 'image', part: file.part };
  }
  const target = selection.mode === 'image' ? 'the object matching `reference`' : 'the red circle';
  const russian = selection.mode === 'ru';
  const request = {
    model, state, assets,
    questions: {
      presence: { type: 'noul', instructions: russian
        ? 'Есть ли на изображении `scene` красный круг?' : `Is ${target} visible in \`scene\`?` },
      count: { type: 'choice', instructions: russian
        ? 'Сколько красных кругов видно на изображении `scene`?' : `How many instances of ${target} are visible in \`scene\`?`,
      criteria: russian
        ? { none: 'Ни одного', one: 'Ровно один', several: 'Несколько', uncertain: 'Невозможно определить' }
        : { none: 'No matching target', one: 'Exactly one', several: 'More than one', uncertain: 'Cannot determine' } },
      location: { type: 'box', instructions: russian
        ? 'Красный круг на изображении `scene`' : `${target} in \`scene\`` },
    },
  };
  const body = new FormData();
  body.append('request', new Blob([JSON.stringify(request)], { type: 'application/json' }), 'request.json');
  for (const file of files) body.append(file.part, new Blob([file.bytes], { type: 'image/png' }), `${file.name}.png`);
  return {
    url: endpoint, body,
    context: { case_id: caseId, scene_file: selection.file, scene_sha256: scene.info.sha256, source_crop: scene.info.source_crop },
    summary: { case_id: caseId, model, image_count: files.length, image_bytes: files.reduce((sum, file) => sum + file.bytes.length, 0) },
  };
}

const probability = value => typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 1;
const integer = value => Number.isSafeInteger(value) && value >= 0;

export function validateResponse(payload, context, currentSceneHash, thresholds) {
  const usage = payload?.usage;
  const measured = usage && integer(usage.input_tokens) && integer(usage.output_tokens)
    ? { input_tokens: usage.input_tokens, output_tokens: usage.output_tokens } : null;
  const result = { kind: 'invalid', source_box: null, highlight_permitted: false, usage: measured };
  const presence = payload?.answers?.presence;
  const count = payload?.answers?.count;
  const location = payload?.answers?.location;
  const box = location?.box;
  const options = ['none', 'one', 'several', 'uncertain'];
  if (payload?.model !== model || presence?.type !== 'noul' || !probability(presence.noul) ||
      count?.type !== 'choice' || !options.includes(count.choice) || !probability(count.confidence) ||
      !count.probabilities || Object.keys(count.probabilities).sort().join() !== [...options].sort().join() ||
      !options.every(key => probability(count.probabilities[key])) ||
      Math.abs(Object.values(count.probabilities).reduce((a, b) => a + b, 0) - 1) > 1e-6 ||
      count.probabilities[count.choice] !== Math.max(...Object.values(count.probabilities)) ||
      location?.type !== 'box' || !probability(location.confidence) || !box ||
      !['left', 'top', 'right', 'bottom'].every(key => probability(box[key])) ||
      box.left >= box.right || box.top >= box.bottom) return result;
  result.evidence = { presence: presence.noul, count: count.choice, probabilities: count.probabilities, box, box_confidence: location.confidence };
  if (currentSceneHash !== context.scene_sha256) return { ...result, kind: 'stale' };
  if (![thresholds?.presence_min, thresholds?.unique_min].every(value => probability(value) && value > 0.5)) {
    return { ...result, kind: 'unverified' };
  }
  const confidentCount = count.probabilities[count.choice] >= thresholds.unique_min;
  if (presence.noul <= 1 - thresholds.presence_min && count.choice === 'none' && confidentCount) {
    return { ...result, kind: 'absent' };
  }
  if (presence.noul < thresholds.presence_min || !confidentCount) return { ...result, kind: 'uncertain' };
  if (count.choice === 'several') return { ...result, kind: 'ambiguous' };
  if (count.choice !== 'one') return { ...result, kind: 'uncertain' };
  const [left, top, right, bottom] = context.source_crop;
  // Coordinates belong to this exact sent crop; a candidate never authorizes native highlighting.
  return { ...result, kind: 'candidate', source_box: [
    Math.round(left + box.left * (right - left)), Math.round(top + box.top * (bottom - top)),
    Math.round(left + box.right * (right - left)), Math.round(top + box.bottom * (bottom - top)),
  ] };
}

export function preflight({ caseId = 'P-en', purpose, settings, apiKey, ledgerPath = sharedLedger } = {}) {
  const blockers = [];
  let plan = null;
  let upper = null;
  let budget = { cap_nanousd: 1000000000, committed_nanousd: null, remaining_nanousd: null };
  try { plan = buildRequest(caseId).summary; } catch { blockers.push('invalid_fixture_or_case'); }
  if (typeof apiKey !== 'string' || !apiKey.trim() || /[\r\n]/.test(apiKey)) blockers.push('missing_api_key');
  if (settings?.live_enabled !== true) blockers.push('live_disabled');
  if (!validPurpose(purpose)) blockers.push('missing_test_purpose');
  try {
    upper = parseUsd(settings?.upper_bound_usd);
    if (upper <= 0 || typeof settings?.upper_bound_evidence !== 'string' || !settings.upper_bound_evidence.trim()) throw new Error();
  } catch { upper = null; blockers.push('unconfirmed_upper_bound'); }
  if (typeof settings?.account_terms_reference !== 'string' || !settings.account_terms_reference.trim()) blockers.push('unconfirmed_account_terms');
  try {
    budget = budgetStatus(ledgerPath);
    if (budget.bound_breached) blockers.push('upper_bound_breached');
    if (upper !== null && upper > budget.remaining_nanousd) blockers.push('insufficient_budget');
  } catch { blockers.push('missing_or_invalid_shared_ledger'); }
  const warning = upper !== null && budget.committed_nanousd !== null
    ? warningForExposure(budget.committed_nanousd + upper) : null;
  return { status: blockers.length ? 'blocked' : 'ready', network_calls: 0, blockers, plan, budget, warning };
}

async function boundedJSON(response) {
  const reader = response.body?.getReader();
  if (!reader) throw new Error('missing_response_body');
  const chunks = [];
  let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > 65536) { await reader.cancel(); throw new Error('oversize_response'); }
      chunks.push(Buffer.from(value));
    }
  } finally { reader.releaseLock(); }
  return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks)));
}

export async function runAttempt({ caseId, purpose, settings, apiKey, ledgerPath = sharedLedger, testURL, timeoutMs = 90000 }) {
  let url = endpoint;
  // The only endpoint override is an isolated localhost test with an explicitly fake credential.
  if (testURL !== undefined) {
    try {
      const parsed = new URL(testURL);
      if (parsed.protocol !== 'http:' || parsed.hostname !== '127.0.0.1' || parsed.username || parsed.password || apiKey !== 'test-only-key') throw new Error();
      url = parsed.href;
    } catch { return { status: 'blocked', reason: 'invalid_loopback_test' }; }
  } else if (ledgerPath !== sharedLedger) return { status: 'blocked', reason: 'noncanonical_live_ledger' };
  const admission = preflight({ caseId, purpose, settings, apiKey, ledgerPath });
  if (admission.status !== 'ready') return admission;
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 90000) return { status: 'blocked', reason: 'invalid_timeout' };
  let prepared;
  let attemptId;
  try {
    prepared = buildRequest(caseId);
    attemptId = reserve(ledgerPath, caseId, settings.upper_bound_usd, settings.upper_bound_evidence,
      { purpose, model, endpoint: url });
  } catch { return { status: 'blocked', reason: 'invalid_request_or_budget_admission' }; }
  const started = performance.now();
  try {
    const response = await fetch(url, {
      method: 'POST', redirect: 'error', signal: AbortSignal.timeout(timeoutMs), body: prepared.body,
      headers: { Authorization: `Bearer ${apiKey}`, 'TypeSafe-Danger-Zone-Images': 'true' },
    });
    if (!response.ok) {
      await response.body?.cancel();
      return { status: 'unknown', attempt_id: attemptId, http_status: response.status, elapsed_ms: Math.round(performance.now() - started) };
    }
    const payload = await boundedJSON(response);
    const currentHash = fixture(prepared.context.scene_file).info.sha256;
    return { status: 'unreconciled', attempt_id: attemptId, elapsed_ms: Math.round(performance.now() - started),
      classification: validateResponse(payload, prepared.context, currentHash, settings) };
  } catch {
    // A disconnect, timeout or malformed response cannot prove the provider did not charge us.
    return { status: 'unknown', attempt_id: attemptId, elapsed_ms: Math.round(performance.now() - started) };
  }
}

async function main() {
  const [mode = 'preflight', caseId = 'P-en', amount, receipt] = process.argv.slice(2);
  if (mode === 'plan') {
    console.log(JSON.stringify({ status: 'prepared_offline', network_calls: 0, cases: Object.keys(cases).map(id => buildRequest(id).summary) }, null, 2));
    return;
  }
  let settings;
  try { settings = JSON.parse(readFileSync(settingsURL, 'utf8')); } catch { /* Missing or damaged settings deny admission. */ }
  let result;
  if (mode === 'init-ledger') {
    initializeLedger(sharedLedger, settings?.opening_exposure_usd, settings?.opening_exposure_evidence);
    result = budgetStatus(sharedLedger);
  } else if (mode === 'reconcile') {
    settle(sharedLedger, caseId, amount, receipt);
    result = budgetStatus(sharedLedger);
  } else if (mode === 'budget') result = budgetStatus(sharedLedger);
  else if (mode === 'run') result = await runAttempt({ caseId, purpose: amount, settings, apiKey: process.env.TYPESAFE_API_KEY });
  else if (['preflight', 'dry-run'].includes(mode)) {
    result = { ...preflight({ caseId, purpose: amount, settings, apiKey: process.env.TYPESAFE_API_KEY }),
      settings_path: fileURLToPath(settingsURL), shared_ledger_path: sharedLedger, api_key_environment: 'TYPESAFE_API_KEY' };
  } else throw new Error('unknown_mode');
  console.log(JSON.stringify(result, null, 2));
  if (result.status === 'blocked' || result.status === 'unknown') process.exitCode = 2;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(() => { console.log(JSON.stringify({ status: 'blocked', reason: 'invalid_local_configuration_or_ledger' })); process.exitCode = 2; });
}

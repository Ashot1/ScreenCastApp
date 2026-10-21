// One test-only ledger for every Jev feasibility task and wave, never a product budget.
import { DatabaseSync } from 'node:sqlite';
import { openSync, closeSync, existsSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { randomUUID } from 'node:crypto';

const cap = 1000000000;
const evidence = value => typeof value === 'string' && value.trim().length > 0 && value.length <= 1000;
export const validPurpose = value => typeof value === 'string' && value.trim().length > 0 && Buffer.byteLength(value) <= 1000 && !/[\r\n]/.test(value);

export function parseUsd(value) {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)(\.[0-9]{1,9})?$/.test(value)) throw new Error('unknown_or_invalid_money');
  const [whole, fraction = ''] = value.split('.');
  const amount = BigInt(whole) * 1000000000n + BigInt(fraction.padEnd(9, '0'));
  if (amount > BigInt(Number.MAX_SAFE_INTEGER)) throw new Error('money_out_of_range');
  return Number(amount);
}

export function initializeLedger(path, openingUsd, openingEvidence) {
  const opening = parseUsd(openingUsd);
  if (!evidence(openingEvidence)) throw new Error('missing_opening_reconciliation');
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  // Exclusive creation forbids a new wave from resetting unresolved earlier attempts.
  closeSync(openSync(path, 'wx', 0o600));
  const db = new DatabaseSync(path);
  try {
    db.exec(`PRAGMA synchronous=FULL;
      CREATE TABLE campaign (id INTEGER PRIMARY KEY CHECK(id=1), cap INTEGER NOT NULL, opening INTEGER NOT NULL CHECK(opening>=0), evidence TEXT NOT NULL);
      CREATE TABLE attempts (id TEXT PRIMARY KEY, case_id TEXT NOT NULL, reserved INTEGER NOT NULL CHECK(reserved>0), bound_evidence TEXT NOT NULL, charged INTEGER CHECK(charged>=0), receipt TEXT, purpose TEXT, model TEXT, endpoint TEXT);
    `);
    db.prepare('INSERT INTO campaign VALUES(1,?,?,?)').run(cap, opening, openingEvidence);
  } finally { db.close(); }
}

function withLedger(path, action) {
  if (!existsSync(path)) throw new Error('missing_shared_ledger');
  const db = new DatabaseSync(path);
  try {
    db.exec('PRAGMA busy_timeout=5000; PRAGMA synchronous=FULL;');
    let campaign;
    db.exec('BEGIN IMMEDIATE');
    try {
      campaign = db.prepare('SELECT * FROM campaign WHERE id=1').get();
      if (![cap, 2500000000].includes(campaign?.cap) || !Number.isSafeInteger(campaign.opening) || campaign.opening < 0 || !evidence(campaign.evidence)) {
        throw new Error('invalid_shared_ledger');
      }
      // Lower the old authorization without losing any expense or inventing missing historical audit facts.
      const columns = new Set(db.prepare('PRAGMA table_info(attempts)').all().map(column => column.name));
      for (const name of ['purpose', 'model', 'endpoint']) {
        if (!columns.has(name)) db.exec(`ALTER TABLE attempts ADD COLUMN ${name} TEXT`);
      }
      if (campaign.cap > cap) db.prepare('UPDATE campaign SET cap=? WHERE id=1').run(cap);
      campaign.cap = cap;
      db.exec('COMMIT');
    } catch (error) { db.exec('ROLLBACK'); throw error; }
    return action(db, campaign);
  } finally { db.close(); }
}

function status(db, campaign) {
  const attempts = db.prepare('SELECT COALESCE(SUM(COALESCE(charged,reserved)),0) AS exposure, MAX(charged>reserved) AS breached FROM attempts').get();
  const committed = campaign.opening + attempts.exposure;
  if (!Number.isSafeInteger(committed)) throw new Error('invalid_shared_ledger');
  return { cap_nanousd: cap, committed_nanousd: committed, remaining_nanousd: cap - committed, bound_breached: attempts.breached === 1 };
}

export function budgetStatus(path) {
  return withLedger(path, (db, campaign) => ({ ...status(db, campaign),
    attempts: db.prepare('SELECT * FROM attempts ORDER BY rowid').all(),
  }));
}

export function warningForExposure(exposure) {
  return exposure >= 800000000 ? {
    kind: 'budget_warning_80_percent', cap_nanousd: cap,
    committed_with_reservations_nanousd: exposure,
    basis: 'charges_and_unsettled_reservations_including_new_attempt',
  } : null;
}

export function reserve(path, caseId, upperUsd, boundEvidence, audit) {
  const amount = parseUsd(upperUsd);
  if (amount <= 0 || typeof caseId !== 'string' || !caseId.trim() || caseId.length > 100) throw new Error('invalid_reservation');
  if (!evidence(boundEvidence)) throw new Error('missing_upper_bound_evidence');
  if (![audit?.purpose, audit?.model, audit?.endpoint].every(validPurpose)) throw new Error('missing_attempt_audit');
  const reservation = withLedger(path, (db, campaign) => {
    db.exec('BEGIN IMMEDIATE');
    try {
      const current = status(db, campaign);
      if (current.bound_breached) throw new Error('upper_bound_breached');
      if (amount > current.remaining_nanousd) throw new Error('insufficient_budget');
      const id = randomUUID();
      db.prepare('INSERT INTO attempts(id,case_id,reserved,bound_evidence,purpose,model,endpoint) VALUES(?,?,?,?,?,?,?)')
        .run(id, caseId, amount, boundEvidence, audit.purpose, audit.model, audit.endpoint);
      db.exec('COMMIT');
      return { id, warning: warningForExposure(current.committed_nanousd + amount) };
    } catch (error) { db.exec('ROLLBACK'); throw error; }
  });
  // Only the committed atomic admission emits; preflight describes the same warning without emitting it.
  if (reservation.warning) console.warn(JSON.stringify(reservation.warning));
  return reservation.id;
}

export function settle(path, id, chargedUsd, receipt) {
  const amount = parseUsd(chargedUsd);
  if (!evidence(receipt)) throw new Error('missing_billing_receipt');
  return withLedger(path, db => {
    // Compare-and-set keeps the first confirmed bill immutable, including bills above the bound.
    const changed = db.prepare('UPDATE attempts SET charged=?,receipt=? WHERE id=? AND charged IS NULL').run(amount, receipt, id);
    if (changed.changes !== 1) throw new Error('already_settled');
  });
}

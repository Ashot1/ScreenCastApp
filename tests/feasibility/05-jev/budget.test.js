import { afterEach, describe, it, expect } from 'vitest';
import { mkdtempSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { DatabaseSync } from 'node:sqlite';
import { initializeLedger, reserve, settle, budgetStatus, parseUsd } from './budget.mjs';

const audit = { purpose: 'Check an unverified synthetic criterion', model: 'jev-1.14.0', endpoint: 'https://api.typesafe.ai/v1/systemone' };
const hold = (path, caseId, usd) => reserve(path, caseId, usd, 'synthetic upper-bound evidence', audit);

const directories = [];
function ledger(opening = '0') {
  const directory = mkdtempSync(join(tmpdir(), 'jev-budget-test-'));
  directories.push(directory);
  const path = join(directory, 'ledger.sqlite3');
  initializeLedger(path, opening, 'synthetic test-only opening reconciliation');
  return path;
}
afterEach(() => { for (const directory of directories.splice(0)) rmSync(directory, { recursive: true, force: true }); });

describe('shared Jev test cap', () => {
  it('requires explicit opening exposure and never silently creates or overwrites a ledger', () => {
    const path = ledger('0.4');
    expect(() => initializeLedger(path, '0', 'reset')).toThrow();
    expect(() => initializeLedger(`${path}-missing`, null, 'unknown prior spend')).toThrow();
    expect(() => budgetStatus(`${path}-missing`)).toThrow();
    expect(existsSync(`${path}-missing`)).toBe(false);
    expect(budgetStatus(path)).toMatchObject({ cap_nanousd: 1000000000, committed_nanousd: 400000000, remaining_nanousd: 600000000 });
  });
  it('keeps unknown attempts across reopen and charges every new retry against the same cap', () => {
    const path = ledger();
    const attempt = hold(path, 'P-en', '0.6');
    expect(budgetStatus(path).remaining_nanousd).toBe(400000000);
    expect(() => hold(path, 'retry-or-another-wave', '0.6')).toThrow('insufficient_budget');
    expect(() => settle(path, attempt, '0.1', '')).toThrow();
    settle(path, attempt, '0.1', 'synthetic billing receipt');
    hold(path, 'another-task', '0.6');
    expect(budgetStatus(path).remaining_nanousd).toBe(300000000);
    expect(() => settle(path, attempt, '0', 'replacement receipt')).toThrow('already_settled');
  });
  it('records a breached bound and freezes further admission instead of hiding an unexpected bill', () => {
    const path = ledger();
    const attempt = hold(path, 'P-en', '0.1');
    settle(path, attempt, '0.2', 'synthetic unexpected invoice');
    expect(budgetStatus(path)).toMatchObject({ bound_breached: true, committed_nanousd: 200000000 });
    expect(() => hold(path, 'P-ru', '0.1')).toThrow('upper_bound_breached');
  });
  it('reserves atomically between independent processes', async () => {
    const path = ledger();
    const script = `import { reserve } from ${JSON.stringify(new URL('./budget.mjs', import.meta.url).href)};
      try { reserve(process.argv[1], 'concurrent', '0.6', 'synthetic upper bound', ${JSON.stringify(audit)}); console.log('reserved'); }
      catch { console.log('blocked'); }`;
    const results = await Promise.all([1, 2].map(() => promisify(execFile)(process.execPath, ['--input-type=module', '-e', script, path])));
    expect(results.map(result => result.stdout.trim()).sort()).toEqual(['blocked', 'reserved']);
    expect(budgetStatus(path).committed_nanousd).toBe(600000000);
  });
  it('rejects unknown, non-finite or rounded money instead of assuming zero', () => {
    for (const value of [null, undefined, 'NaN', 'Infinity', '-1', '0.0000000001', 0.1]) {
      expect(() => parseUsd(value)).toThrow();
    }
    expect(parseUsd('0.000000042')).toBe(42);
  });
  it('exposes unresolved attempt IDs after reopening and requires bound evidence', () => {
    const path = ledger();
    expect(() => reserve(path, 'P-en', '0.1')).toThrow('missing_upper_bound_evidence');
    expect(() => reserve(path, 'P-en', '0.1', 'synthetic bound')).toThrow('missing_attempt_audit');
    const id = hold(path, 'P-en', '0.1');
    expect(budgetStatus(path).attempts).toEqual([expect.objectContaining({ id, case_id: 'P-en', reserved: 100000000, charged: null,
      bound_evidence: 'synthetic upper-bound evidence', ...audit })]);
  });
  it('lowers an existing cap without discarding old actual charges or unknown reserves', () => {
    const directory = mkdtempSync(join(tmpdir(), 'jev-budget-test-'));
    directories.push(directory);
    const path = join(directory, 'ledger.sqlite3');
    const db = new DatabaseSync(path);
    db.exec(`CREATE TABLE campaign(id INTEGER PRIMARY KEY,cap INTEGER,opening INTEGER,evidence TEXT);
      INSERT INTO campaign VALUES(1,2500000000,200000000,'old opening reconciliation');
      CREATE TABLE attempts(id TEXT PRIMARY KEY,case_id TEXT,reserved INTEGER,bound_evidence TEXT,charged INTEGER,receipt TEXT);
      INSERT INTO attempts VALUES('older-unknown','P-en',900000000,'old bound',NULL,NULL);
      INSERT INTO attempts VALUES('older-paid','A-en',100000000,'old bound',50000000,'old receipt');`);
    db.close();
    const preserved = budgetStatus(path);
    expect(preserved).toMatchObject({ cap_nanousd: 1000000000, committed_nanousd: 1150000000, remaining_nanousd: -150000000 });
    expect(preserved.attempts).toHaveLength(2);
    expect(preserved.attempts[0]).toMatchObject({ id: 'older-unknown', charged: null, reserved: 900000000, purpose: null, model: null, endpoint: null });
    expect(() => hold(path, 'new-attempt', '0.01')).toThrow('insufficient_budget');
    expect(budgetStatus(path).attempts).toHaveLength(2);
  });
});

import { spawnSync } from 'node:child_process';
import { lstatSync, readFileSync, realpathSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { parseTrustOperationJsonl } from './agent-team-trust-ledger.mjs';

function commandJson(binary, args) {
  const result = spawnSync(binary, args, { encoding: 'utf8', timeout: 30000, maxBuffer: 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`trusted CLI ${args.slice(0, 2).join(' ')} failed`);
  try { return JSON.parse(result.stdout); } catch { throw new Error('trusted CLI returned invalid JSON'); }
}
function completeRows(text, label) {
  return text.slice(0, text.lastIndexOf('\n') + 1).split('\n').filter(line => line.trim()).map((line, i) => {
    let row;
    try { row = JSON.parse(line); } catch { throw new Error(`${label} malformed complete row ${i + 1}`); }
    if (!row || typeof row !== 'object' || Array.isArray(row)) throw new Error(`${label} invalid row ${i + 1}`);
    return row;
  });
}
// Callbacks are module-level dependency injection for isolated tests, never
// read from evidence JSON or exposed as native-root/receipt CLI options.
export function loadSelectedSpaceSources(spaceId, ledgerPath, binary = 'harness', runJson = commandJson) {
  if (typeof spaceId !== 'string' || !spaceId.trim() || spaceId.startsWith('-')) throw new Error('explicit trusted Space id required');
  const space = runJson(binary, ['space', 'show', spaceId]);
  if (space?.id !== spaceId || typeof space.store_root !== 'string') throw new Error('selected Space identity/root mismatch');
  const root = realpathSync(space.store_root);
  const canonicalLedger = join(root, 'agentfirm_trust_operations.jsonl');
  if (!ledgerPath || resolve(ledgerPath) !== canonicalLedger) throw new Error('--trust-ledger must equal selected Space canonical ledger path');
  const read = name => {
    const path = join(root, name);
    if (!lstatSync(path).isFile() || realpathSync(path) !== path) throw new Error(`selected Space source ${name} is not a regular canonical file`);
    return readFileSync(path, 'utf8');
  };
  return {
    records: parseTrustOperationJsonl(read('agentfirm_trust_operations.jsonl')),
    teamRuns: completeRows(read('team_runs.jsonl'), 'TeamRun history'),
    // Managed-only Spaces may not have a lease file. Missing external history
    // stays an explicit refusal in attribution verification.
    hostLeases: (() => { try { return completeRows(read('host_binding_leases.jsonl'), 'Host lease history'); }
      catch (error) { if (error.code === 'ENOENT') return []; throw error; } })(),
    validateHost: (surface, thread) => runJson(binary,
      ['team-run', 'validate-host-session', '--surface', surface, '--thread-id', thread]),
  };
}

// The review round's check (verification.toml `round`), run at the head under
// review: the whitespace check over the branch's changes, then the evidence
// check of every receipt folder the branch changed (e2e/receipts/<name>/
// check-evidence.mjs), so a receipt's binding to that head is run by the
// server, not only claimed. Run from the repository root:
//   node scripts/round-check.mjs [base]
// The base defaults to the merge base of HEAD and the remote's default branch.
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

const RECEIPTS = 'e2e/receipts/';
const CHECKER = 'check-evidence.mjs';
// Records the agent host writes into a checkout, never the task's work.
const HARVESTED = ['.wiki/decisions/', '.wiki/memory/'];

/** The checkers to run: one per receipt folder a changed path lies in, where
 * that folder has a checker at HEAD (`tracked`), sorted, each once. */
export function receiptChecks(changed, tracked) {
  const has = new Set(tracked);
  const folders = changed
    .filter((p) => p.startsWith(RECEIPTS))
    .map((p) => p.slice(RECEIPTS.length).split('/'))
    .filter((parts) => parts.length > 1 && parts[0])
    .map((parts) => `${RECEIPTS}${parts[0]}/${CHECKER}`);
  return [...new Set(folders)].filter((c) => has.has(c)).sort();
}

function git(...args) {
  const r = spawnSync('git', args, { encoding: 'utf8' });
  return { ok: r.status === 0, out: (r.stdout ?? '').trim(), err: (r.stderr ?? '').trim() };
}

function defaultBranch() {
  const local = git('rev-parse', '--abbrev-ref', 'origin/HEAD');
  if (local.ok && local.out) return local.out;
  const remote = git('ls-remote', '--symref', 'origin', 'HEAD');
  const m = remote.out.match(/^ref: refs\/heads\/(\S+)\s+HEAD/m);
  return m ? `origin/${m[1]}` : null;
}

function main() {
  const named = process.argv[2];
  const branch = named ? null : defaultBranch();
  const base = named ?? (branch && git('merge-base', branch, 'HEAD').out);
  if (!base) {
    console.error('round-check: no base: name one, or give origin a default branch');
    return 2;
  }
  console.log(`round-check: base ${base}${branch ? ` (merge base with ${branch})` : ''}, head ${git('rev-parse', 'HEAD').out}`);
  const own = git('config', '--get', 'core.whitespace').out;
  const whitespace = ['-c', `core.whitespace=${[own, 'cr-at-eol'].filter(Boolean).join(',')}`, 'diff', '--check', base, 'HEAD', '--', '.', ...HARVESTED.map((h) => `:(exclude)${h}`)];
  console.log(`$ git ${whitespace.join(' ')}`);
  if (spawnSync('git', whitespace, { stdio: 'inherit' }).status !== 0) return 1;
  const changed = git('-c', 'core.quotepath=off', 'diff', '--name-only', '--no-renames', base, 'HEAD').out.split('\n').filter(Boolean);
  const tracked = git('-c', 'core.quotepath=off', 'ls-files', '--', `${RECEIPTS}*/${CHECKER}`).out.split('\n').filter(Boolean);
  const checks = receiptChecks(changed, tracked);
  if (!checks.length) console.log('round-check: no receipt folder changed');
  let failed = 0;
  for (const c of checks) {
    console.log(`$ node --test ${c}`);
    failed += spawnSync(process.execPath, ['--test', c], { stdio: 'inherit' }).status !== 0;
  }
  console.log(failed ? `round-check: ${failed} of ${checks.length} receipt checks failed` : `round-check: passed (${checks.length} receipt checks)`);
  return failed ? 1 : 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) process.exit(main());

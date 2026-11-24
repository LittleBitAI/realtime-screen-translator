// Ties this receipt to the reviewed head: every file is read from HEAD at its
// pinned hash, the runtime code equals the candidate the runs used, and the
// reports and the Gemini ledger hold what verification.txt says.
// Run from the repository root: node --test e2e/receipts/balloon-fit/check-evidence.mjs
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';

const folder = 'e2e/receipts/balloon-fit';
const source = '88c3bd717ba837e67a751e060987e50461c7a02f';
const run = source.slice(0, 7);
const pinned = {
  'api-probe.mjs': '578a89edc76aea7a94ac5855986b1d1c841b2382b71cbdcf91e0148cc629727b',
  'api-probe.txt': '922480f02f538a2461f21a5fa3efd5daec496fa7ee866091e02df4e006a1c2da',
  'capture-hashes.json': '70dfe6b5c9174c151bdac957054ea0b4fd57e6ceee3e496214fbfd488996dc9e',
  'cdp.mjs': '069109bd7e1203ed032cb891e73a7dc4f18f9eb9f0780d9fff33d95c68d71f57',
  'crops.json': 'c087eeaa342fd7ec1f608d94c35bdd2944691ce84c7ef0253f5d66cd396d76f2',
  'edge-walk-report.json': '21fcfa199f2501f400ce2ec88b37d1f83cc62070c71638089e276cce4379a8f8',
  'edge-walk.mjs': '5dc1570f4fbd92ef459e2cf55dd49376849c7ec7bde513cb9c5de8f5ef78ad61',
  'emulator-88c3bd7-ch256-p09-before-after.png': 'b9f7b6dbb93a5334c0c9fa7c7a5bf5fd3991e15114ce15480fa5df198d22e49c',
  'emulator-88c3bd7-ch257-p06-before-after.png': '1b16a4b37dce19c18038899fe90823f6ba583b6e2ec57470331bbdb0665a3fd3',
  'emulator-88c3bd7-ch258-p17-before-after.png': 'a2ccd8cd686f555ebfaa24fe72ddf98fb874093db6b85cd68dac609c80563306',
  'emulator-88c3bd7-ch258-p18-before-after.png': '218e52bec592dd65edbe419f27d7406f74099063080d0d6e3c9c3e5edc498eeb',
  'emulator-88c3bd7-fill-report.json': '28c8747132eb9e178acaeb6d530eecf2fcd671f6bc039ec4e806d6d4fb7dfff5',
  'emulator-88c3bd7-report.json': '1ffa190f473e2770bcd8a48c81f360bb0cde85349b41601c1b51016d1fb9f209',
  'focused-tests.txt': 'd145ab6c2a8305da05cfcc3df88e700e51a1ff5ddc92df08fa8b66270c23b09c',
  'leak-report.txt': 'db82a0a8a35f52dc301a920dce0e177649e59f0ba3ec6e3e266f4ceac1012cad',
  'ledger-approvals.json': 'fb50ac1370ae00e61522e592ee46c7e6a26eebf2036116a95fd95b30351b6ea3',
  'ledger-attempts.jsonl': '1cd02b312e4934d7df4b8dddf530e871fd7b4f4eb91c683a1ce3706631466575',
  'ledger-decisions.json': '34e95c992a477ad9dd546975dbab0da388cf845b20a1eacbc782512a3347e3f9',
  'ledger-pages.json': '7d5b0889a8275516bc76b56b75b58d26b50aa61b105b47b60eeedd6d85d64f89',
  'phone-walk.mjs': '8a7ad25e9b23b15a6c5719bcda897b53c6f3e2f6b0e6d89eb154bd42aaa1588d',
  'replay-layouts.json': '355a627e27905c6ffb37747fde06596853ec59d192b7a9ce670328efe3c937e2',
  's21-883938c-ch256-p12-before-after.png': '96f20d3e5f784ae7e4523454e8ff2ad96847b7daad2da89fa4322b333cf417d9',
  's21-883938c-ch257-p15-before-after.png': '1c397c7a97a0228aae340e49b47e7c8d8b9d1457b09f09612ca2af64fcd21e10',
  's21-883938c-ch258-p14-before-after.png': 'ec5e5d925152b4ebd9109c3ad854e4399d13bcda6ca4a9e229efefd6a2f8c6b2',
  's21-883938c-report.json': '8ff0fefe65f83f87364efaa5b39291c4587ecdc2da2e6c168e0165a0ff154307',
  'windows-88c3bd7-ch257-p05-06-before-after.png': '3ecb150e8dad995d37e8810cff3b49fcc67286c8addb1a42a7c7931415478c42',
  'windows-88c3bd7-ch258-p18-before-after.png': '681e1e00908e0118e4851c16aa990a6ab1f37af1de572eb15345ab1de406d44f',
  'windows-88c3bd7-fill-report.json': '630742a8f53e16e23cd10cef2b53cfd3ba766806892641bab08f88dd67d1e390',
  'windows-88c3bd7-report.json': '005edd22a0e85a013dc25b47f37c42e19ef51324d0562a275959ef3fdd0020e5',
};
const git = (...args) => execFileSync('git', args, { maxBuffer: 64 << 20 });
const read = (name) => git('show', `HEAD:${folder}/${name}`);
const json = (name) => JSON.parse(read(name).toString('utf8'));

test('every file of the receipt is pinned, named in it and read from HEAD at its hash', () => {
  const tracked = git('ls-tree', '--name-only', `HEAD:${folder}`).toString('utf8').trim().split('\n');
  const own = ['verification.txt', 'check-evidence.mjs'];
  assert.deepEqual(tracked.filter((f) => !own.includes(f)).sort(), Object.keys(pinned).sort());
  const receipt = read('verification.txt').toString('utf8');
  for (const [name, sha256] of Object.entries(pinned)) {
    assert.ok(receipt.includes(name), `verification.txt names ${name}`);
    assert.equal(createHash('sha256').update(read(name)).digest('hex'), sha256, name);
  }
});

test('the runtime code equals the candidate the runs used', () => {
  git('diff', '--exit-code', '--quiet', source, 'HEAD', '--', '.',
    `:(exclude,glob)${folder}/**`, ':(exclude,glob)docs/research/**', ':(exclude,glob).wiki/**');
});

const walks = [`emulator-${run}-fill-report.json`, `emulator-${run}-report.json`, `windows-${run}-fill-report.json`, `windows-${run}-report.json`];

test('the walks drew every page of the three chapters, the read-throughs with no Gemini request', () => {
  const pages = { 9439528: 21, 9478775: 21, 9494751: 19 };
  for (const name of [...walks, 's21-883938c-report.json']) {
    const r = json(name);
    assert.deepEqual(Object.fromEntries(r.chapters.map((c) => [c.url.split('/').pop(), c.pages])), pages, name);
    assert.ok(r.chapters.every((c) => c.chips === 0 && c.detail.every((p) => p.picture && p.boxes > 0)), name);
  }
  // Every capture pair of this head's walks shows the switch: none retaken
  // to the end with translation on and off alike.
  for (const name of walks) assert.ok(json(name).chapters.every((c) => c.same_captures === 0), name);
  for (const name of [`emulator-${run}-report.json`, `windows-${run}-report.json`, 's21-883938c-report.json']) {
    const r = json(name);
    assert.equal(r.gemini_requests_after, r.gemini_requests_before, name);
  }
  const edge = json('edge-walk-report.json');
  assert.deepEqual(Object.values(edge).map((c) => [c.total, c.drawn, c.inOrder, c.clipped, c.unpainted]), [[21, 21, true, 0, 0], [21, 21, true, 0, 0], [19, 19, true, 0, 0]]);
});

test('the reused S21 walk covers only pages whose layout this head left unchanged, and each changed page has its pair', () => {
  const r = json('replay-layouts.json');
  assert.equal(r.source_commit, run);
  assert.equal(r.gemini_requests, 0);
  assert.equal(r.reopened.kept, 61);
  assert.equal(Object.keys(r.pages).length, 61);
  const changed = Object.entries(r.pages).filter(([, p]) => !p.same_as_883938c).map(([k]) => k);
  assert.equal(changed.length, 18);
  const crops = json('crops.json');
  for (const c of crops) assert.equal(createHash('sha256').update(read(c.file)).digest('hex'), c.sha256, c.file);
  const shown = new Set(crops.filter((c) => c.file.startsWith(`emulator-${run}-`)).flatMap((c) => c.pages.map((p) => `${c.file.split('-')[2]}/${String(p.page).padStart(2, '0')}.json`)));
  for (const k of ['ch256', 'ch257', 'ch258']) assert.ok(changed.some((p) => p.startsWith(k) && shown.has(p)), `a changed ${k} page is shown`);
});

// Each further attempt for a page (the second on) must have gone out on a
// grant dated before it, from a decision of the person's that names that
// page and was answered at the grant's time. The problems, none if so.
function unapproved(attempts, approvals, decisions, pages) {
  const byId = Object.fromEntries(decisions.map((d) => [d.id, d]));
  const sent = {};
  const problems = [];
  for (const a of attempts.filter((a) => a.kind === 'outbound')) {
    const n = (sent[a.key] = (sent[a.key] ?? 0) + 1);
    if (n === 1) continue;
    const page = pages[a.key.split(':')[0]];
    const grants = Array.isArray(approvals[a.key]) ? approvals[a.key] : [];
    const before = grants.filter((g) => Date.parse(g.at) < Date.parse(a.at)).sort((x, y) => Date.parse(x.at) - Date.parse(y.at));
    const grant = before[n - 2];
    if (!grant) { problems.push(`${page.chapter} p${page.page} attempt ${n} at ${a.at}: ${before.length} grant(s) before it`); continue; }
    for (const id of String(grant.decision ?? '').split('+')) {
      const d = byId[id];
      if (!d || d.answered_at !== grant.at || !d.pages.some(([c, p]) => c === page.chapter && p === page.page)) {
        problems.push(`${page.chapter} p${page.page} attempt ${n}: grant ${grant.at} from ${id || 'no decision'} does not name it at that time`);
      }
    }
  }
  return problems;
}

test('a repeat request counts as approved only on a grant dated before it from a decision naming its page', () => {
  const pages = { aa: { chapter: 'ch1', page: 1 }, bb: { chapter: 'ch1', page: 2 } };
  const decisions = [{ id: 'D1', answered_at: '2026-01-01T10:00:00Z', pages: [['ch1', 1]] }];
  const sent = (at) => [{ kind: 'outbound', key: 'aa:en', at: '2026-01-01T09:00:00Z' }, { kind: 'outbound', key: 'aa:en', at }];
  const forms = [
    ['a grant answered before the repeat', sent('2026-01-01T10:05:00Z'), { 'aa:en': [{ at: '2026-01-01T10:00:00Z', decision: 'D1' }] }, true],
    ['a grant dated after the repeat', sent('2026-01-01T09:30:00Z'), { 'aa:en': [{ at: '2026-01-01T10:00:00Z', decision: 'D1' }] }, false],
    ['no grant', sent('2026-01-01T10:05:00Z'), {}, false],
    ['a bare count', sent('2026-01-01T10:05:00Z'), { 'aa:en': 1 }, false],
    ['a grant with no decision', sent('2026-01-01T10:05:00Z'), { 'aa:en': [{ at: '2026-01-01T10:00:00Z' }] }, false],
    ['a decision unknown', sent('2026-01-01T10:05:00Z'), { 'aa:en': [{ at: '2026-01-01T10:00:00Z', decision: 'D9' }] }, false],
    ['a grant at another time than its decision', sent('2026-01-01T10:05:00Z'), { 'aa:en': [{ at: '2026-01-01T09:59:00Z', decision: 'D1' }] }, false],
  ];
  for (const [name, attempts, approvals, ok] of forms) assert.equal(unapproved(attempts, approvals, decisions, pages).length === 0, ok, name);
  const other = [{ kind: 'outbound', key: 'bb:en', at: '2026-01-01T09:00:00Z' }, { kind: 'outbound', key: 'bb:en', at: '2026-01-01T10:05:00Z' }];
  assert.equal(unapproved(other, { 'bb:en': [{ at: '2026-01-01T10:00:00Z', decision: 'D1' }] }, decisions, pages).length, 1, 'a decision naming another page');
});

test('every repeat Gemini attempt for a page went out on the person\'s prior approval', () => {
  const attempts = read('ledger-attempts.jsonl').toString('utf8').trim().split('\n').map((l) => JSON.parse(l));
  const pages = json('ledger-pages.json');
  assert.deepEqual(unapproved(attempts, json('ledger-approvals.json'), json('ledger-decisions.json'), pages), []);
  const keys = new Set(attempts.filter((a) => a.kind === 'outbound').map((a) => a.key.split(':')[0]));
  assert.equal(keys.size, Object.keys(pages).length);
});

test('the focused checks and the API probe passed', () => {
  const t = read('focused-tests.txt').toString('utf8');
  assert.match(t, new RegExp(`source commit ${source}`));
  assert.match(t, /Summary \[[^\]]*\] (\d+) tests run: \1 passed, 0 skipped/);
  assert.match(t, /Tests\s+21 passed \(21\)/);
  assert.doesNotMatch(t, /FAIL|×/);
  const probe = read('api-probe.txt').toString('utf8').trim().split('\n');
  assert.equal(probe.at(-1), 'PASS 2 cases');
  for (const c of probe.slice(0, -1).map((l) => JSON.parse(l))) {
    assert.equal(c.observed, c.expected, c.case);
    assert.equal(c.request.path, '/v1beta/models/gemini-3.1-flash-lite:generateContent', c.case);
    assert.equal(c.request.prefill, '{"items":[', c.case);
  }
});

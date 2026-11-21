// Ties this receipt to the reviewed head: every file is read from HEAD at its
// pinned hash, the runtime code equals the candidate the runs used, and the
// reports and the Gemini ledger hold what verification.txt says.
// Run from the repository root: node --test e2e/receipts/balloon-fit/check-evidence.mjs
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';

const folder = 'e2e/receipts/balloon-fit';
const source = 'ad06fcfd5985796b1b09fc4e431dec2725ecfeda';
const pinned = {
  'capture-hashes.json': '330e311400358439671b0cd98a8d280148f5e5f16ff231e638521f17d17fcc2b',
  'cdp.mjs': '95694de31a31f0e18595fa00b3bfa6a20a56fb87bf45572654adf4f14a32c39f',
  'edge-walk-report.json': '85b9d9c0d645656bd41d81c1449014aa73ecb16f16482002cb01e4e9aba5b2f4',
  'edge-walk.mjs': '84293ea4281a4fdcee147cfdccf8c349f6a1d035442dc54d6646544f88aa44fc',
  'emulator-ad06fcf-ch256-p08-before-after.png': '7957595a4c77ec2fd3232c35bcaa9d6eb5df719a226584b2edffe7fe7da9158a',
  'emulator-ad06fcf-ch257-p04-before-after.png': '2394e8901d77c33d31e4aa53f6b293e2a67288530501fff1915e3b1d9dd755d0',
  'emulator-ad06fcf-fill-report.json': '61f163574a2060758bd700441e7d2da52047ead3d796cfa8633b02597ab202fe',
  'emulator-ad06fcf-report.json': '317f14a433cf869c97a38aab93c75538ff02f9a31dc5aa949e69c1a83cd6a67d',
  'fix-ch256-p08-source-883938c-ad06fcf.png': '5330adcd23bcdb288d5ca24b11119add339caac14ff74a2e63c54774232a477a',
  'fix-ch257-p04-source-883938c-ad06fcf.png': '34145bf74e8d65c95cd25849b679c862311df50c01680cf3df64820e3e6b4197',
  'focused-tests.txt': '13af7f88a04a2fdfc12833117aaf6fa8d303f3eae685527da51b7def0f08ebcf',
  'leak-report.txt': 'b56aa2c0b6ecc0f5458d7e32454c2f4763fb80921c50ca047921f49b4abe633b',
  'ledger-approvals.json': '6b1d33f1dbbb17a3ec7ca22eaf32de7ccdda5b774afc3631e16f8ca0b12cbeca',
  'ledger-attempts.jsonl': '8e520a333b3a4b7006fbee760f62ca2bd49ad521129cd40202a9303ad4a02ce7',
  'ledger-pages.json': '7d5b0889a8275516bc76b56b75b58d26b50aa61b105b47b60eeedd6d85d64f89',
  'phone-walk.mjs': '443499e8f6b632244dbae920b5aac1e7ed63e3044597361796e3ac27356a7e87',
  'replay-layouts.json': 'df26edb584f2c56008073d049f6409275d1d1cae8be5e9810a703958b87666df',
  's21-883938c-ch256-p12-before-after.png': '96f20d3e5f784ae7e4523454e8ff2ad96847b7daad2da89fa4322b333cf417d9',
  's21-883938c-ch257-p15-before-after.png': '1c397c7a97a0228aae340e49b47e7c8d8b9d1457b09f09612ca2af64fcd21e10',
  's21-883938c-ch258-p14-before-after.png': 'ec5e5d925152b4ebd9109c3ad854e4399d13bcda6ca4a9e229efefd6a2f8c6b2',
  's21-883938c-report.json': '8ff0fefe65f83f87364efaa5b39291c4587ecdc2da2e6c168e0165a0ff154307',
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

test('the walks drew every page of the three chapters with no Gemini request', () => {
  const pages = { 9439528: 21, 9478775: 21, 9494751: 19 };
  for (const name of ['emulator-ad06fcf-report.json', 'emulator-ad06fcf-fill-report.json', 's21-883938c-report.json']) {
    const r = json(name);
    assert.deepEqual(Object.fromEntries(r.chapters.map((c) => [c.url.split('/').pop(), c.pages])), pages, name);
    assert.ok(r.chapters.every((c) => c.chips === 0 && c.detail.every((p) => p.picture && p.boxes > 0)), name);
  }
  for (const name of ['emulator-ad06fcf-report.json', 's21-883938c-report.json']) {
    const r = json(name);
    assert.equal(r.gemini_requests_after, r.gemini_requests_before, name);
  }
  const edge = json('edge-walk-report.json');
  assert.deepEqual(Object.values(edge).map((c) => [c.total, c.drawn, c.inOrder, c.clipped, c.unpainted]), [[21, 21, true, 0, 0], [21, 21, true, 0, 0], [19, 19, true, 0, 0]]);
});

test('the reused S21 walk covers only pages whose layout the candidate left unchanged', () => {
  const r = json('replay-layouts.json');
  assert.equal(r.gemini_requests, 0);
  assert.equal(Object.keys(r.pages).length, 61);
  const changed = Object.entries(r.pages).filter(([, p]) => !p.same_as_883938c).map(([k]) => k);
  assert.deepEqual(changed, ['ch256/08.json', 'ch257/04.json']);
  assert.ok(read('emulator-ad06fcf-ch256-p08-before-after.png').length && read('emulator-ad06fcf-ch257-p04-before-after.png').length);
});

test('every repeat Gemini attempt for a page was approved', () => {
  const attempts = read('ledger-attempts.jsonl').toString('utf8').trim().split('\n').map((l) => JSON.parse(l));
  const approvals = json('ledger-approvals.json');
  const sent = {};
  for (const a of attempts.filter((a) => a.kind === 'outbound')) sent[a.key] = (sent[a.key] ?? 0) + 1;
  for (const [key, n] of Object.entries(sent)) assert.ok(n - 1 <= (approvals[key] ?? 0), `${key}: ${n} sent`);
  assert.equal(Object.keys(sent).length, Object.keys(json('ledger-pages.json')).length);
});

test('the focused checks passed', () => {
  const t = read('focused-tests.txt').toString('utf8');
  assert.match(t, /Summary 217 tests run: 217 passed, 0 skipped/);
  assert.match(t, /Tests\s+14 passed \(14\)/);
  assert.doesNotMatch(t, /FAIL|×/);
});

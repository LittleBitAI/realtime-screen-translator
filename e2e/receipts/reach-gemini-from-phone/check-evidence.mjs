import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';

const folder = 'e2e/receipts/reach-gemini-from-phone';
const reviewedSource = '27209bf40ecace8be53f4ae83ebb28c0f34204ba';
const firstReview = 'abcf7884d3a19917abc6b7fe093a517fdea24741';
const fixedSource = '7e5876fba15c845e3de332a7d650433f2e937de8';
const phoneSource = 'dda9640ad4d0cc3b4170391ac3bf178f28d95850';
const receiptPath = `:(exclude,glob)${folder}/**`;
const pinned = {
  'desktop-reader.test.mjs': '25fff41858a7f9aa93395448bab34c629d75dc8386ec14e9f8edb03a4ccf7e98',
  'focused-checks.txt': 'e3c5393403e85757064a0d6e3d4bd0db8d1200be71ba2ba7a6ab97e2f1197155',
  'round-2-focused-checks.txt': '8b387089e51341c6b7e0df744ee1528dd6a5789eb499203261d43ee72050b764',
  'round-2-windows-desktop.json': '8086505dabfed43a2de7d99859afe59806d998e69618bfecf7aece133bb753ee',
  'verification.txt': '6b93dac1ede9eb606c402ee5b79e1b68ce2d5423d2665053b372174d4069213f',
  'windows-desktop.json': 'a1479e7edce5f2f7120034c90f7d94dc9fabf978f9f29b2eb90177cda7e95d08',
  'windows-read-to-end.png': '88104f93795face54d5fb99fdbe31149bbd2192c6db0093b0c14dfb68c948e1e',
  'windows-reopened.png': 'd044072b0fa56c04172f50ac125be72c8ba92b2129cfb74a3f681f6d12e22547',
};

const git = (...args) => execFileSync('git', args);
const read = name => git('show', `HEAD:${folder}/${name}`);

test('the focused reader run and new native builds use the candidate inputs', () => {
  git('diff', '--exit-code', '--no-renames', fixedSource, 'HEAD', '--', '.', receiptPath);
});

test('the historical Galaxy proof matches the first review candidate inputs', () => {
  git('diff', '--exit-code', '--no-renames', phoneSource, firstReview, '--', '.', receiptPath,
    ':(exclude,literal).wiki/.sync', ':(exclude,literal).wiki/corpus.json');
});

test('the Rust inputs from the first review are identical for the reused focused run', () => {
  git('diff', '--exit-code', '--no-renames', reviewedSource, 'HEAD', '--',
    'Cargo.toml', 'Cargo.lock', '.cargo', 'src-tauri', ':(exclude,literal)src-tauri/src/reader.js');
});

test('the review evidence is committed at the recorded hashes', () => {
  for (const [file, hash] of Object.entries(pinned)) {
    assert.equal(createHash('sha256').update(read(file)).digest('hex'), hash, file);
  }
});

test('the focused results cover the reader and capture regressions at the tested source', () => {
  const reused = read('focused-checks.txt').toString('utf8');
  assert.ok(reused.startsWith(`Reviewed source: ${reviewedSource}\n`));
  assert.match(reused, /^Summary .* 34 tests run: 34 passed, 199 skipped$/m);
  const current = read('round-2-focused-checks.txt').toString('utf8');
  assert.ok(current.startsWith(`Source: ${fixedSource}\n`));
  assert.match(current, /^FAIL enforces the received page-list size limit for XHR json at 2000001 characters$/m);
  assert.match(current, /^Test Files  8 passed \(8\)$/m);
  assert.match(current, /^Tests  65 passed \(65\)$/m);
});

test('the native reader completed in place and all three pages reopened without a request', () => {
  const report = JSON.parse(read('round-2-windows-desktop.json').toString('utf8'));
  const { initial, reopened } = report;
  assert.equal(report.sourceCommit, fixedSource);
  assert.equal(report.environment.platform, 'win32');
  assert.equal(report.environment.architecture, 'x64');
  assert.equal(report.build.sha256, '28ebd80ea1f14f564fe9072748b235369505673e94a07da8195849e1e55f5b70');
  assert.notEqual(initial.pid, reopened.pid);
  assert.equal(initial.beforeRun.scroll, initial.afterRun.scroll);
  assert.deepEqual([initial.beforeRun.pictureCount, initial.afterRun.pictureCount], [1, 1]);
  assert.equal(initial.afterRun.status, '회차 끝까지 번역했습니다.');
  assert.deepEqual(initial.laterMounted.visibleKorean, [{ page: 2, text: '안녕' }]);
  assert.deepEqual(initial.status, { keySet: true, requests: 3 });
  assert.deepEqual(reopened.requestOrder, [1, 2, 3]);
  for (const status of [reopened.before, reopened.after]) assert.deepEqual(status, { keySet: true, requests: 0 });
  assert.deepEqual(reopened.pages.map(p => p.visibleKorean), [1, 2, 3].map(page => [{ page, text: '안녕' }]));
  assert.ok(reopened.pages.every(p => p.runPressed === 'false' && !p.status && !p.chips.length));
});

test('the native reader ignored an oversized decoded JSON body while the site received it', () => {
  const report = JSON.parse(read('round-2-windows-desktop.json').toString('utf8'));
  assert.deepEqual(report.fixture.oversizedJsonXhr, {
    status: 200, type: 'json', decodedBytes: 2_000_001, addresses: 4, encodedBytes: 2039, extraPictureRequests: 0,
  });
});

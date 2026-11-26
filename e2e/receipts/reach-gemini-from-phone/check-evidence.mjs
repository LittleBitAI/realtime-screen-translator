import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';

const folder = 'e2e/receipts/reach-gemini-from-phone';
const reviewedSource = '27209bf40ecace8be53f4ae83ebb28c0f34204ba';
const phoneSource = 'dda9640ad4d0cc3b4170391ac3bf178f28d95850';
const receiptPath = `:(exclude,glob)${folder}/**`;
const pinned = {
  'desktop-reader.test.mjs': '99b2f4b46d0c9ee9f7975fa72cd79beb5fa056cccfe2ccdf979b822d44cbaa9c',
  'focused-checks.txt': 'e3c5393403e85757064a0d6e3d4bd0db8d1200be71ba2ba7a6ab97e2f1197155',
  'verification.txt': '04bcd3ab68f8afd32aef28cfd5c679f419e88ed8a0ae00cdbe7470b8ead51436',
  'windows-desktop.json': 'a1479e7edce5f2f7120034c90f7d94dc9fabf978f9f29b2eb90177cda7e95d08',
  'windows-read-to-end.png': '88104f93795face54d5fb99fdbe31149bbd2192c6db0093b0c14dfb68c948e1e',
  'windows-reopened.png': 'd044072b0fa56c04172f50ac125be72c8ba92b2129cfb74a3f681f6d12e22547',
};

const git = (...args) => execFileSync('git', args);
const read = name => git('show', `HEAD:${folder}/${name}`);

test('the offline and desktop runs use the candidate inputs', () => {
  git('diff', '--exit-code', '--no-renames', reviewedSource, 'HEAD', '--', '.', receiptPath);
});

test('the reused Galaxy S21 proof uses the candidate runtime inputs', () => {
  git('diff', '--exit-code', '--no-renames', phoneSource, 'HEAD', '--', '.', receiptPath,
    ':(exclude,literal).wiki/.sync', ':(exclude,literal).wiki/corpus.json');
});

test('the review evidence is committed at the recorded hashes', () => {
  for (const [file, hash] of Object.entries(pinned)) {
    assert.equal(createHash('sha256').update(read(file)).digest('hex'), hash, file);
  }
});

test('the focused results cover the reader and capture regressions at the tested source', () => {
  const focused = read('focused-checks.txt').toString('utf8');
  assert.ok(focused.startsWith(`Reviewed source: ${reviewedSource}\n`));
  assert.match(focused, /^Summary .* 34 tests run: 34 passed, 199 skipped$/m);
  assert.match(focused, /^Test Files  8 passed \(8\)$/m);
  assert.match(focused, /^Tests  55 passed \(55\)$/m);
});

test('the native reader completed in place and all three pages reopened without a request', () => {
  const report = JSON.parse(read('windows-desktop.json').toString('utf8'));
  const { initial, reopened } = report;
  assert.equal(report.sourceCommit, reviewedSource);
  assert.equal(report.environment.platform, 'win32');
  assert.equal(report.environment.architecture, 'x64');
  assert.equal(report.build.sha256, 'e7ceb8d2b7e3ebec65f7b79b917a5cb16501d7566c8ece125406f5840cc62b9a');
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

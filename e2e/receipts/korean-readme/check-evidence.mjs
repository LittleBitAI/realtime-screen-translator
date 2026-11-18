import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';

const root = new URL('../../../', import.meta.url);
const folder = 'e2e/receipts/korean-readme';
const git = (...args) => execFileSync('git', args, { cwd: root });
const receipt = git('show', 'HEAD:' + folder + '/verification.txt').toString('utf8');
const field = (label) => {
  const line = receipt.split(/\r?\n/).find((line) => line.startsWith(label + ': '));
  assert.ok(line, 'Missing receipt field ' + label);
  return line.slice(label.length + 2);
};

test('the tested guide and installer inputs match the checked-out commit', () => {
  const blob = field('Tested README blob');
  const tested = field('Walkthrough checkout');
  assert.equal(git('rev-parse', tested + ':README.md').toString().trim(), blob);
  assert.equal(git('rev-parse', 'HEAD:README.md').toString().trim(), blob);
  git('diff', '--exit-code', field('Installer source'), 'HEAD', '--', '.',
    ':(exclude,literal)README.md',
    ':(exclude,literal).wiki/.sync',
    ':(exclude,literal).wiki/corpus.json',
    ':(exclude,literal).wiki/graph.json',
    ':(exclude,glob)e2e/receipts/windows-installer/**',
    ':(exclude,glob)e2e/receipts/korean-readme/**');
});

test('the guide starts with reader instructions and its repository links resolve', () => {
  const bytes = git('show', 'HEAD:README.md');
  assert.notDeepEqual(bytes.subarray(0, 3), Buffer.from([0xef, 0xbb, 0xbf]));
  const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  const headings = Array.from(text.matchAll(/^#{2,3} (.+)$/gm), ([, heading]) =>
    heading.toLowerCase().replaceAll(' ', '-'));
  assert.ok(headings.includes('개발-환경'));
  assert.ok(headings.slice(0, headings.indexOf('개발-환경')).includes('첫-회차-번역해서-읽기'));
  for (const [, target] of text.matchAll(/\]\(([^)]+)\)/g)) {
    if (target.startsWith('#')) assert.ok(headings.includes(target.slice(1)), target);
    else if (target.startsWith('docs/')) git('cat-file', '-e', 'HEAD:' + target);
  }
});

test('all four Windows captures are tracked and match their receipt', () => {
  const captures = Array.from(receipt.matchAll(/^([\w-]+\.png) ([a-f0-9]{64})$/gm));
  assert.deepEqual(captures.map(([, name]) => name).sort(), [
    'installed.png', 'reopened-chapter.png', 'saved-list.png', 'url-reader.png',
  ]);
  for (const [, name, digest] of captures) {
    const bytes = git('show', 'HEAD:' + folder + '/' + name);
    assert.equal(createHash('sha256').update(bytes).digest('hex'), digest, name);
  }
});

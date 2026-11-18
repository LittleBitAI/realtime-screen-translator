import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';

const folder = 'e2e/receipts/windows-installer';
const captures = [
  ['installed-launch.png', '25773dfa998c5e0ccbdf6b76b3c37a139e4a847c6e5706ab7e5e6f31e2f2f07c'],
  ['native-folder-picker.png', '9f37e8e6b37844a3d44fddb7ddb14af9a2aa0dc0727aed329a2e928191e95748'],
  ['reopened-chapter-native.png', '187ef6cd0717e8836a1582b9eaec05fa912e8e428f1bb6f55d2a78ba21d70426'],
  ['saved-list-after-restart.png', '5086243d0170bec19b53a1c3b6e6cde4e1ec1db38214bc8bfcc7470bd406ae2a'],
  ['url-reader.png', '5c6d6c903d7fb4cb4d93b1dbbf1527c29ca309e07a9c6373f4e836330f7c253d'],
];
const git = (...args) => execFileSync('git', args);

test('every cited desktop capture is readable from HEAD', () => {
  const receipt = git('show', `HEAD:${folder}/verification.txt`).toString('utf8');
  const listed = receipt.match(/\S+\.png\b/g) ?? [];
  assert.deepEqual(listed.sort(), captures.map(([name]) => name));
  for (const [name, sha256] of captures) {
    const bytes = git('show', `HEAD:${folder}/${name}`);
    assert.equal(createHash('sha256').update(bytes).digest('hex'), sha256, name);
  }
});

test('the installer build inputs match the reviewed tree', () => {
  git('diff', '--exit-code', '4c3390cdeda0af426912cf789a8f0c9f0d4879e3', 'HEAD', '--', '.',
    `:(exclude,glob)${folder}/**`);
});

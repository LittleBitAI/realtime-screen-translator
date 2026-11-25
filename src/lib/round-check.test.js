// @vitest-environment node
// The review round's check (scripts/round-check.mjs): which receipt checkers
// a branch's changed paths call for.
import { describe, expect, it } from 'vitest';
import { receiptChecks } from '../../scripts/round-check.mjs';

const tracked = ['e2e/receipts/alpha/check-evidence.mjs', 'e2e/receipts/beta/check-evidence.mjs'];
const alpha = 'e2e/receipts/alpha/check-evidence.mjs';
const beta = 'e2e/receipts/beta/check-evidence.mjs';

describe('round check', () => {
  // Property: a receipt folder's checker runs exactly when a changed path lies
  // inside that folder and the folder has a checker at the head; each once.
  it.each([
    ['a report changed in a folder with a checker', ['e2e/receipts/alpha/report.json'], [alpha]],
    ['the checker itself changed', [alpha], [alpha]],
    ['a picture deeper in the folder', ['e2e/receipts/alpha/shots/one.png'], [alpha]],
    ['several files of one folder', ['e2e/receipts/alpha/a.txt', 'e2e/receipts/alpha/b.txt'], [alpha]],
    ['two folders, listed out of order', ['e2e/receipts/beta/a.txt', 'e2e/receipts/alpha/a.txt'], [alpha, beta]],
    ['a folder with no checker', ['e2e/receipts/gamma/a.txt'], []],
    ['a file at the receipts root', ['e2e/receipts/notes.txt'], []],
    ['a sibling folder named like it', ['e2e/receipts-old/alpha/a.txt'], []],
    ['runtime code only', ['src/app.js', 'startup.rc'], []],
    ['no change', [], []],
  ])('%s', (_, changed, want) => {
    expect(receiptChecks(changed, tracked)).toEqual(want);
  });
});

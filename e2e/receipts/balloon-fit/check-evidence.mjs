// Ties this receipt to the reviewed head: every file is read from HEAD at its
// pinned hash, the runtime code equals the candidate the runs used, and the
// reports and the Gemini ledger hold what verification.txt says.
// Run from the repository root: node --test e2e/receipts/balloon-fit/check-evidence.mjs
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';

const folder = 'e2e/receipts/balloon-fit';
const source = '2d94efe399491be2c2998ad0cff5f0fe10c666cb';
const run = source.slice(0, 7);
const pinned = {
  'api-probe.mjs': '2763a52d76b47fa60c94554d075520c412293ef25c517a2626017268fafe9997',
  'api-probe.txt': 'd2120e7b8a8cbd5730608ae4bc8ecb6b1b229dcd70de301233fb6eac6d114776',
  'capture-hashes.json': '91d8b4805d368cca629efb2de198e00e3ccfbd8b7740cda72e68704a5e9a1004',
  'cdp.mjs': '069109bd7e1203ed032cb891e73a7dc4f18f9eb9f0780d9fff33d95c68d71f57',
  'crops.json': '84898468fdd10094af55c877ae2d6d4609576b7d31367402fac31d580d53001c',
  'edge-walk-report.json': 'd885f9e00df4881f317e5f041d4eef86efad5806b2a4e9ff62bdf6a54c2adf83',
  'edge-walk.mjs': 'c2d06852f3d5924bd4644928993d24e0a79c53e2d9e3105d82bc7dfdb37cf94b',
  'emulator-2d94efe-ch256-p12-before-after.png': '691ec2c87501e6247f8fba6a1fe742c183081b1d183595d0760b04072bdab6c4',
  'emulator-2d94efe-ch256-p21-before-after.png': '31f0f6eee5d81be2196eb0aa28b95ce095f5bd6382e50277b3ac65aa83840edc',
  'emulator-2d94efe-ch257-p02-before-after.png': '413f3cd19a42918d659bfc202fe429ae866aab41f4d03b2311d8db7e73630cc1',
  'emulator-2d94efe-ch257-p04-before-after.png': '19689e7d7dcdf5cd5ad67331972dae5def594b7499ca511b9b2c5d68a9d8425f',
  'emulator-2d94efe-ch257-p16-before-after.png': 'ed7b5f61b76c5cb5eba12ea7ee2805a511c75025b114ecbe0627d0e5b2b85d4f',
  'emulator-2d94efe-ch257-p19-before-after.png': 'fc2db99cd15dcd37971b162f81332716daea55cc2341bcf7d8a893be0919fbfc',
  'emulator-2d94efe-ch258-p11-before-after.png': '9dbd881a0de6709883cdff1780f71699cab28cca0e0e6c2e109c3256ca0b5f28',
  'emulator-2d94efe-ch258-p18-before-after.png': '57e116290de2758c98676843d177b558b1d95340d6c55cceceeac4b3f26036fb',
  'emulator-2d94efe-fill-report.json': '42f2e132e4fa1ddb4ce4a0ecdeffb60f4f978edf380273ea002f856564170b53',
  'emulator-2d94efe-report.json': '2c83353b7cc4ddfb40a6c4828a93097233ccd86caae55edcdf9faa47db1472ee',
  'fix-2d94efe-ch257-p04-off-before-after.png': '8406ceacf83a4a494b75679dd6d75dbc4b7f844a48a7427f9f0e2a2d03231ea8',
  'fix-2d94efe-ch258-p11-off-before-after.png': 'b268922c77c990af5863dca88779100ce251f71c1ed11ad6e7782fa73b7a5599',
  'focused-tests.txt': '9566d80c24dc78f6e272a2c8bd9d2faf7b6a359bf6bec1c081f317bf039ca8e8',
  'leak-accepted.json': 'c4be974790f0e20fbba4206279a88b090d795b0de816bb747158c84c00836f30',
  'leak-check.txt': '29e125d1492dab8f26b50aa82c40effc23af1e39b89c8209a55aaeb7366caa18',
  'ledger-approvals.json': 'fb50ac1370ae00e61522e592ee46c7e6a26eebf2036116a95fd95b30351b6ea3',
  'ledger-attempts.jsonl': 'a0283b330d4842b9e33ce6a1f11be2ac4f4946a4f64e8b05bdc27a89e455f7e4',
  'ledger-decisions.json': '34e95c992a477ad9dd546975dbab0da388cf845b20a1eacbc782512a3347e3f9',
  'ledger-pages.json': '7d5b0889a8275516bc76b56b75b58d26b50aa61b105b47b60eeedd6d85d64f89',
  'phone-walk.mjs': '889bd3fc54cd76aec454129fc89d8c06c831d34803a68282c9b1b021c81912c6',
  'replay-layouts.json': 'e2b448f5c8dcfb1fb98aea21f7c3c604367f06a32936c65141c737171b30927f',
  's21-883938c-ch256-p12-before-after.png': '96f20d3e5f784ae7e4523454e8ff2ad96847b7daad2da89fa4322b333cf417d9',
  's21-883938c-ch257-p15-before-after.png': '1c397c7a97a0228aae340e49b47e7c8d8b9d1457b09f09612ca2af64fcd21e10',
  's21-883938c-ch258-p14-before-after.png': 'ec5e5d925152b4ebd9109c3ad854e4399d13bcda6ca4a9e229efefd6a2f8c6b2',
  's21-883938c-report.json': '8ff0fefe65f83f87364efaa5b39291c4587ecdc2da2e6c168e0165a0ff154307',
  'windows-2d94efe-ch257-p19-20-before-after.png': '4b460b28a24177d3ee2409e15f8b6e68df9d27cb5cdca73a93c014c5e23cc28d',
  'windows-2d94efe-ch258-p18-before-after.png': 'f360f5cacd2b0f449cbbaf7630138539b094719b824aa1ba92c4bc7560fe84ed',
  'windows-2d94efe-fill-report.json': '04b4575f0ec68edf630d88f181617cc15d6cb999bf5893bb5010a871f490ea26',
  'windows-2d94efe-report.json': 'fea045f3f3c046612716b870b8a02d2cf35683eb07c274c3a21ffb0b49e8ef25',
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

// Left out: this evidence, research notes, the server's wiki metadata, and
// the review round's own check (verification.toml and scripts/round-check.mjs
// with its test), none of which the app or the walks load.
test('the runtime code equals the candidate the runs used', () => {
  git('diff', '--exit-code', '--quiet', source, 'HEAD', '--', '.',
    `:(exclude,glob)${folder}/**`, ':(exclude,glob)docs/research/**', ':(exclude,glob).wiki/**',
    ':(exclude,literal)verification.toml', ':(exclude,literal)scripts/round-check.mjs', ':(exclude,literal)src/lib/round-check.test.js');
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

test('no Korean leaves its white cover\'s text area or is cut off, and none on the art is drawn below its least size', () => {
  // Edge: the size drawn (set size times any scale) against the least size.
  for (const [ch, c] of Object.entries(json('edge-walk-report.json'))) {
    assert.deepEqual([c.belowLeast, c.clipped, c.pastWhite, c.whitePastArea], [0, 0, 0, 0], ch);
    assert.ok(c.pages.every((p) => p.belowLeast === 0 && p.pastCover.every((x) => !x.white) && Array.isArray(p.whitePastArea) && p.whitePastArea.length === 0), ch);
  }
  // The installed apps: every text measured; none cut; in a white cover
  // inside its own text area (scaled below the least size there if need
  // be, the person's choice); on the art never below its least size.
  for (const name of walks) {
    for (const c of json(name).chapters) {
      const texts = c.detail.flatMap((p) => p.texts ?? []);
      assert.ok(texts.length > 0, `${name} ${c.url}: texts measured`);
      assert.ok(texts.every((t) => typeof t.white === 'boolean' && typeof t.past_area === 'boolean'), `${name} ${c.url}: each text's cover and area read`);
      assert.deepEqual([c.cut, c.past_white, c.white_past_area], [0, 0, 0], `${name} ${c.url}`);
      assert.ok(texts.every((t) => !t.cut && (t.white ? !t.past_area && t.past !== 'white' : t.px >= t.least - 0.05)), `${name} ${c.url}`);
    }
  }
});

test('every tone cover on the site is drawn at the share of its box set, none bent by the site\'s own sheet', () => {
  for (const name of walks) {
    const chapters = json(name).chapters;
    assert.ok(chapters.every((c) => c.detail.every((p) => p.chip || Number.isInteger(p.patches))), `${name}: every page's patches counted`);
    assert.ok(chapters.reduce((a, c) => a + c.patches, 0) > 0, `${name}: tone covers measured`);
    for (const c of chapters) assert.equal(c.patches_off, 0, `${name} ${c.url}`);
  }
});

test('the reused S21 walk stands only for pages this head draws as it drew them, and each changed page has its pair', () => {
  const r = json('replay-layouts.json');
  assert.equal(r.source_commit, run);
  assert.equal(r.gemini_requests, 0);
  assert.equal(r.reopened.kept, 61);
  assert.equal(Object.keys(r.pages).length, 61);
  assert.ok(Object.values(r.pages).every((p) => p.same_as_reopened), 'every page reopens to the same layout');
  const changed = Object.entries(r.pages).filter(([, p]) => !p.same_as_883938c).map(([k]) => k);
  assert.equal(changed.length, 22);
  // A page the S21 drew at 883938c stands only where its layout is the same
  // and no tone cover reaches past its box: the S21 drew those squeezed by
  // the site's canvas cap, and the emulator walk is their evidence.
  for (const [k, p] of Object.entries(r.pages)) assert.equal(p.real_s21_stands, p.same_as_883938c && p.covers_wider_than_box === 0, k);
  assert.equal(Object.values(r.pages).filter((p) => p.real_s21_stands).length, 19);
  // Nor where this head's reader draws a balloon line on Android otherwise
  // than at 883938c: scaled below the least size to stay inside its area.
  const ids = { 9439528: 'ch256', 9478775: 'ch257', 9494751: 'ch258' };
  const smaller = json(`emulator-${run}-report.json`).chapters.flatMap((c) => c.detail
    .filter((p) => (p.texts ?? []).some((t) => t.white && t.px < t.least - 0.05))
    .map((p) => `${ids[c.url.split('/').pop()]}/${String(p.page).padStart(2, '0')}.json`));
  assert.ok(smaller.every((k) => r.pages[k] && !r.pages[k].real_s21_stands), smaller.join(' '));
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

// The leak check's dialogue marks left showing (its `SHOWING mark` lines),
// against the marks named in the accepted list, each a sound effect or the
// art beside a line, never dialogue. The problems, none only when the marks
// showing are exactly the named ones and the closing total agrees.
function unaccepted(text, accepted) {
  const lines = text.trim().split('\n');
  const shown = lines.map((l) => l.match(/^(ch\d+) (\d+) SHOWING mark \[(\d+), (\d+), (\d+), (\d+)\]/)).filter(Boolean).map((m) => `${m[1]} ${m[2]} ${m.slice(3, 7).join(',')}`);
  const problems = shown.filter((k) => !Object.hasOwn(accepted, k)).map((k) => `showing and not named: ${k}`);
  for (const [k, why] of Object.entries(accepted)) {
    if (!shown.includes(k)) problems.push(`named and not showing: ${k}`);
    if (!/^(sound effect|art): \S/.test(why)) problems.push(`named as neither a sound effect nor art: ${k}`);
  }
  const total = lines.at(-1).match(/^dialogue marks showing: (\d+)$/);
  if (!total || Number(total[1]) !== shown.length) problems.push(`closing total ${total?.[1] ?? 'missing'} against ${shown.length} marks listed`);
  return problems;
}

test('a leak check passes only when every dialogue mark showing is named as a sound effect or art', () => {
  const page = (shown) => `ch1 01 dialogue ${shown.length}/9\n${shown.map((b) => `ch1 01 SHOWING mark [${b}] glyph 20 of block [0, 0, 99, 99]\n`).join('')}dialogue marks showing: ${shown.length}`;
  const forms = [
    ['no mark showing and none named', page([]), {}, true],
    ['each mark showing named as art or a sound effect', page(['10, 10, 20, 30', '40, 10, 50, 30']), { 'ch1 01 10,10,20,30': 'art: a strand of hair', 'ch1 01 40,10,50,30': 'sound effect: "BANG"' }, true],
    ['a mark showing that is not named', page(['10, 10, 20, 30', '40, 10, 50, 30']), { 'ch1 01 10,10,20,30': 'art: a strand of hair' }, false],
    ['a named mark no longer showing', page([]), { 'ch1 01 10,10,20,30': 'art: a strand of hair' }, false],
    ['a mark named as dialogue', page(['10, 10, 20, 30']), { 'ch1 01 10,10,20,30': 'dialogue: the end of a line' }, false],
    ['a mark named with no reason', page(['10, 10, 20, 30']), { 'ch1 01 10,10,20,30': 'art:' }, false],
    ['a closing total that disagrees', page(['10, 10, 20, 30']).replace(/showing: 1$/, 'showing: 0'), { 'ch1 01 10,10,20,30': 'art: a strand of hair' }, false],
    ['no closing total', page([]).replace(/\ndialogue marks showing: 0$/, ''), {}, false],
  ];
  for (const [name, text, accepted, ok] of forms) assert.equal(unaccepted(text, accepted).length === 0, ok, name);
});

test('the leak check over this head\'s layouts shows no source dialogue: every mark left showing is a named sound effect or art', () => {
  const text = read('leak-check.txt').toString('utf8');
  assert.deepEqual(unaccepted(text, json('leak-accepted.json')), []);
  assert.equal(text.split('\n').filter((l) => /^ch\d+ \d+ dialogue \d+\/\d+$/.test(l)).length, 61, 'every page checked');
});

// The API probe's receipt against what each case sent: the outcome
// expected, the request's model path and prefill, and for a page kept the
// Korean the stand-in answered, as this check knows it (`korean`, by case),
// both as recorded in the answer and as the library gave it back. Text
// decoded in another code page on the way fails here. The problems, none if so.
function probeProblems(text, korean) {
  const lines = text.trim().split('\n');
  const problems = lines.at(-1) === `PASS ${lines.length - 1} cases` ? [] : [`closing line ${lines.at(-1)}`];
  for (const c of lines.slice(0, -1).map((l) => JSON.parse(l))) {
    if (c.observed !== c.expected || !c.pass) problems.push(`${c.case}: observed ${c.observed}`);
    if (c.request?.path !== '/v1beta/models/gemini-3.1-flash-lite:generateContent' || c.request?.prefill !== '{"items":[') problems.push(`${c.case}: request`);
    const want = korean[c.case] ?? null;
    let answered = null;
    try { answered = want && JSON.parse(`{"items":[${c.answered}`).items.map((i) => i.ko); } catch { answered = 'unreadable'; }
    for (const [what, got] of [['answered', answered], ['answered_ko', c.answered_ko ?? null], ['kept', c.kept ?? null]]) {
      if (JSON.stringify(got) !== JSON.stringify(want)) problems.push(`${c.case}: ${what} ${JSON.stringify(got)}, not ${JSON.stringify(want)}`);
    }
  }
  return problems;
}

test('an API probe receipt passes only when a kept page holds the Korean answered, unchanged', () => {
  const korean = { 'a kept page': ['가나'] };
  const line = (o) => JSON.stringify({ case: 'a kept page', request: { path: '/v1beta/models/gemini-3.1-flash-lite:generateContent', prefill: '{"items":[' },
    answered: '{"ko":"가나"}]}', expected: 'kept', observed: 'kept', answered_ko: ['가나'], kept: ['가나'], pass: true, ...o });
  const receipt = (...ls) => `${ls.join('\n')}\nPASS ${ls.length} cases`;
  const forms = [
    ['the Korean kept as answered', receipt(line({})), true],
    ['answer and page both decoded in another code page', receipt(line({ answered: '{"ko":"?섎"}]}', answered_ko: ['?섎'], kept: ['?섎'] })), false],
    ['the page keeping other Korean than answered', receipt(line({ kept: ['가'] })), false],
    ['the page keeping nothing', receipt(line({ kept: null })), false],
    ['no Korean recorded as answered', receipt(line({ answered_ko: undefined })), false],
    ['an outcome other than expected', receipt(line({ observed: 'failed' })), false],
    ['a case marked failed', receipt(line({ pass: false })), false],
    ['a closing line counting other cases', `${line({})}\nPASS 2 cases`, false],
  ];
  for (const [name, text, ok] of forms) assert.equal(probeProblems(text, korean).length === 0, ok, name);
});

test('the focused checks and the API probe passed', () => {
  const t = read('focused-tests.txt').toString('utf8');
  assert.match(t, new RegExp(`source commit ${source}`));
  assert.match(t, /Summary \[[^\]]*\] (\d+) tests run: \1 passed, 0 skipped/);
  assert.match(t, /Tests\s+28 passed \(28\)/);
  assert.doesNotMatch(t, /FAIL|×/);
  // The probe's readable answer carries '안녕'; the repaired one keeps no page.
  assert.deepEqual(probeProblems(read('api-probe.txt').toString('utf8'), { 'readable answer': ['안녕'] }), []);
});

// Ties this receipt to the reviewed head: every file is read from HEAD at its
// pinned hash, the runtime code equals the candidate the runs used, and the
// reports and the Gemini ledger hold what verification.txt says.
// Run from the repository root: node --test e2e/receipts/balloon-fit/check-evidence.mjs
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import test from 'node:test';

const folder = 'e2e/receipts/balloon-fit';
const source = '66b5982c77f257d04390f0885def8bc00b1b685c';
const run = source.slice(0, 7);
const pinned = {
  'api-probe.mjs': '578a89edc76aea7a94ac5855986b1d1c841b2382b71cbdcf91e0148cc629727b',
  'api-probe.txt': 'bd6da3f5a42ab34e2383a47999f93d131d92520041159ca464fb5f96e7d12d2a',
  'capture-hashes.json': 'a0cc5b393b7deadbd2442d3c9aeed889152fa52c06ca632b17eb7d5b3404caa3',
  'cdp.mjs': '069109bd7e1203ed032cb891e73a7dc4f18f9eb9f0780d9fff33d95c68d71f57',
  'crops.json': '442ad919305cb8471976d9d7d4ebc0dd3cbff12f5d9077a41c1973d6f5f79c50',
  'edge-walk-report.json': '97496988809cac312a108aa3439f3b79ff325388e74d71058ed0e2c69940f57a',
  'edge-walk.mjs': '1cea13597d4b597accdda4e11ed0f7f77b8e77ff1567a7050bd7ee89e8945cb6',
  'emulator-66b5982-ch256-p12-before-after.png': '0fee2c20b421505f15cf3f86622b9b7ca8db46dbac8e72872bd7f5ce50fc696a',
  'emulator-66b5982-ch256-p21-before-after.png': 'fa8e75c123ac04ba34759fa62ea0cc7ef44a9773ef326aaec38332361032fbe6',
  'emulator-66b5982-ch257-p02-before-after.png': '950ab294461133141847aed0aaf250352430e21e5202b8a0f517d06382fe3169',
  'emulator-66b5982-ch257-p16-before-after.png': 'e79327476cd61271b8dab42b25e7b902c20d9ed249bea2865e02b4dd3fbddd01',
  'emulator-66b5982-ch257-p19-before-after.png': '0864f0c486aaf01b83f1253eba800c89935dc0de7396d6e97b1086a4c4629461',
  'emulator-66b5982-ch258-p11-before-after.png': 'da021fe3760ff0af0e48e0f53435831ca5451f8d52551ae16a90f7dfb47abfdf',
  'emulator-66b5982-ch258-p18-before-after.png': '11818977124cda11b6dedc8e264a2acd6915f8654ab13ccf8647c92e75a2f763',
  'emulator-66b5982-fill-report.json': '0c62c8b9a51da25b1a88bc62104bcff6db8218758cdfa1c95fbf13227fd74f5e',
  'emulator-66b5982-report.json': 'd567f8c8bd267ccb97a4a8a6b9a60bf546edbfefed850da69aa3f982b820bd45',
  'fix-66b5982-ch256-p12-off-before-after.png': 'df179837d291b698aa5d689a4797144b1c3e6209073e24f685c0b59c4a46dd88',
  'fix-66b5982-ch257-p02-off-before-after.png': '609e7ec9504767c7852aa5cc6f1604a23389cd5b2fd70e80ae5fce48666badf4',
  'fix-66b5982-ch258-p11-off-before-after.png': 'e8f3790775c097e00026256a1e45e8327e9689c47946e9cff2d3f3c3d16cc312',
  'focused-tests.txt': '5d187fc64dc7c8d5cb9428aec92c5706d643b10d4fad0e78d9e9d6efcd873f82',
  'leak-accepted.json': 'c4be974790f0e20fbba4206279a88b090d795b0de816bb747158c84c00836f30',
  'leak-check.txt': '29e125d1492dab8f26b50aa82c40effc23af1e39b89c8209a55aaeb7366caa18',
  'ledger-approvals.json': 'fb50ac1370ae00e61522e592ee46c7e6a26eebf2036116a95fd95b30351b6ea3',
  'ledger-attempts.jsonl': '43f9e8a914bad9f7c812ea07329169e4f96c8e4140e4304821e138a148f3f17c',
  'ledger-decisions.json': '34e95c992a477ad9dd546975dbab0da388cf845b20a1eacbc782512a3347e3f9',
  'ledger-pages.json': '7d5b0889a8275516bc76b56b75b58d26b50aa61b105b47b60eeedd6d85d64f89',
  'phone-walk.mjs': '2209bf2234a22ca7c10ee03716a8dfc22abf9304c2def2e695c244edb62dc978',
  'replay-layouts.json': '0c36abcf78f4683ca4d350bb13aea782e9147acc2a59eca98ac47bcd72590ec8',
  's21-883938c-ch256-p12-before-after.png': '96f20d3e5f784ae7e4523454e8ff2ad96847b7daad2da89fa4322b333cf417d9',
  's21-883938c-ch257-p15-before-after.png': '1c397c7a97a0228aae340e49b47e7c8d8b9d1457b09f09612ca2af64fcd21e10',
  's21-883938c-ch258-p14-before-after.png': 'ec5e5d925152b4ebd9109c3ad854e4399d13bcda6ca4a9e229efefd6a2f8c6b2',
  's21-883938c-report.json': '8ff0fefe65f83f87364efaa5b39291c4587ecdc2da2e6c168e0165a0ff154307',
  'windows-66b5982-ch257-p19-20-before-after.png': 'a34fa01286ac9e8dc9611ea09b23e6f5fe6e7c488d6888fbeb4d692df29ddddf',
  'windows-66b5982-ch258-p18-before-after.png': 'c63ad36f686692147c5c44e35910c1fc255661ee727a14615ef51a99dd66cb91',
  'windows-66b5982-fill-report.json': 'a977b428baf6d773fc6062c876a247648b874d02718c3b77d2afee1a2dc2f0e8',
  'windows-66b5982-report.json': 'f26684a463c565f5f6cba70bed68ec1600e60191332566b34dd72d78cf772f85',
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

test('no Korean is drawn below the page\'s least size, cut off, or past a white cover', () => {
  // Edge: the size drawn (set size times any scale) against the least size.
  for (const [ch, c] of Object.entries(json('edge-walk-report.json'))) {
    assert.deepEqual([c.belowLeast, c.clipped, c.pastWhite], [0, 0, 0], ch);
    assert.ok(c.pages.every((p) => p.belowLeast === 0 && p.pastCover.every((x) => !x.white)), ch);
  }
  // The installed apps: every text measured, none below its least size, cut or past white.
  for (const name of walks) {
    for (const c of json(name).chapters) {
      const texts = c.detail.flatMap((p) => p.texts ?? []);
      assert.ok(texts.length > 0, `${name} ${c.url}: texts measured`);
      assert.deepEqual([c.below_least, c.cut, c.past_white], [0, 0, 0], `${name} ${c.url}`);
      assert.ok(texts.every((t) => t.px >= t.least - 0.05 && !t.cut && t.past !== 'white'), `${name} ${c.url}`);
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

test('the focused checks and the API probe passed', () => {
  const t = read('focused-tests.txt').toString('utf8');
  assert.match(t, new RegExp(`source commit ${source}`));
  assert.match(t, /Summary \[[^\]]*\] (\d+) tests run: \1 passed, 0 skipped/);
  assert.match(t, /Tests\s+26 passed \(26\)/);
  assert.doesNotMatch(t, /FAIL|×/);
  const probe = read('api-probe.txt').toString('utf8').trim().split('\n');
  assert.equal(probe.at(-1), 'PASS 2 cases');
  for (const c of probe.slice(0, -1).map((l) => JSON.parse(l))) {
    assert.equal(c.observed, c.expected, c.case);
    assert.equal(c.request.path, '/v1beta/models/gemini-3.1-flash-lite:generateContent', c.case);
    assert.equal(c.request.prefill, '{"items":[', c.case);
  }
});

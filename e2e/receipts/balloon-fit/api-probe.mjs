// Isolated API probe: the reader backend (src-tauri/examples/replay_chapters.rs,
// the app's own Backend) reads one page of a fresh library against a local
// stand-in for the Gemini API that records each request and answers as told.
// Prints each case's request as received and its expected and observed
// result. Run from the repository root:
//   node e2e/receipts/balloon-fit/api-probe.mjs
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const balloon = (src, ko) => ({ box_2d: [150, 200, 250, 300], balloon_2d: [50, 100, 350, 500], speaker: 'Speaker A', src, ...(ko === undefined ? {} : { ko }) });
// Each case: the text the stand-in continues the prefill with, and what the
// reader must make of it. `reopen`: the same library read again.
const cases = [
  { name: 'readable answer', text: `${JSON.stringify(balloon('HELLO', '안녕'))}]}`, expect: '01: 1 balloons', reopen: '01: 1 balloons (kept)' },
  { name: 'repaired answer with a balloon missing its Korean', text: `${JSON.stringify(balloon('HELLO', '안녕'))},${JSON.stringify(balloon('BYE'))}, {"label": "balloon_2d"`, expect: '01: failed: gemini_unreadable', reopen: null },
];

let requests = [];
let answer = '';
const server = createServer((req, res) => {
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    const body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    const [user, model] = body.contents;
    const image = user.parts.find((p) => p.inline_data)?.inline_data;
    requests.push({
      method: req.method,
      path: req.url,
      key_header: req.headers['x-goog-api-key'] ? 'present' : 'missing',
      roles: body.contents.map((c) => c.role),
      prompt_first_line: user.parts.find((p) => p.text)?.text.split('\n')[0].slice(0, 90),
      image: image && { mime_type: image.mime_type, bytes: Buffer.from(image.data, 'base64').length },
      prefill: model?.parts?.[0]?.text,
      safety: [...new Set(body.safetySettings.map((s) => s.threshold))],
      categories: body.safetySettings.length,
    });
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ candidates: [{ content: { role: 'model', parts: [{ text: answer }] } }] }));
  });
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const base = `http://127.0.0.1:${server.address().port}`;

// Run apart from this process's loop, which answers for the stand-in.
const run = async (data, out, folder) => (await promisify(execFile)('cargo', ['run', '-q', '--release', '--example', 'replay_chapters', '--', data, out, 'en', folder], {
  cwd: 'src-tauri', env: { ...process.env, RST_GEMINI_BASE_URL: base, GEMINI_API_KEY: 'probe-placeholder-key' }, encoding: 'utf8',
})).stdout;

let failed = 0;
for (const c of cases) {
  const root = mkdtempSync(join(tmpdir(), 'api-probe-'));
  const folder = join(root, 'chapter');
  mkdirSync(folder);
  copyFileSync('src-tauri/tests/fixtures/page.jpg', join(folder, '01.jpg'));
  answer = c.text;
  requests = [];
  const first = await run(join(root, 'data'), join(root, 'out'), folder);
  const sent = requests.length;
  const again = c.reopen ? await run(join(root, 'data'), join(root, 'out'), folder) : null;
  const observed = first.split('\n').find((l) => l.startsWith('chapter '))?.replace('chapter ', '');
  const reopened = again?.split('\n').find((l) => l.startsWith('chapter '))?.replace('chapter ', '');
  const ok = observed === c.expect && sent === 1 && (c.reopen === null || (reopened === c.reopen && requests.length === 1));
  failed += !ok;
  console.log(JSON.stringify({ case: c.name, request: requests[0], answered: c.text, expected: c.expect, observed, requests_first: sent,
    ...(c.reopen ? { expected_reopen: c.reopen, observed_reopen: reopened, requests_after_reopen: requests.length } : {}),
    kept: c.reopen ? JSON.parse(readFileSync(join(root, 'out', 'chapter', '01.json'), 'utf8')).boxes.map((b) => b.ko) : null, pass: ok }));
  rmSync(root, { recursive: true, force: true });
}
server.close();
console.log(failed ? `FAIL ${failed}` : `PASS ${cases.length} cases`);
process.exit(failed ? 1 : 0);

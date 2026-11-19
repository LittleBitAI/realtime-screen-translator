// Development proxy between the app and Gemini that holds a request budget.
//
//   node scripts/gemini-ledger.mjs <ledger dir> [port]
//   RST_GEMINI_BASE_URL=http://127.0.0.1:<port>  (default port 8787)
//
// Every request is keyed by the page picture it carries and the source
// language its prompt names. A key whose answer is saved is answered from
// `<ledger>/answers/` with nothing sent out, whatever the prompt around it.
// A new key may go out once; any further attempt for it (the app's own
// retry, the BLOCK_NONE fallback, a reworded prompt) is refused until
// `<ledger>/approvals.json` grants it, as {"<key>": extra attempts}. Every
// attempt, sent or not, is appended to `<ledger>/attempts.jsonl`, and
// GET /ledger sums them up. With LEDGER_OFFLINE=1 nothing is ever sent:
// saved answers are replayed and every other request is refused.
import { createHash } from 'node:crypto';
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';

const [dir, port = '8787'] = process.argv.slice(2);
if (!dir) {
  console.error('usage: node scripts/gemini-ledger.mjs <ledger dir> [port]');
  process.exit(2);
}
const UPSTREAM = process.env.LEDGER_UPSTREAM ?? 'https://generativelanguage.googleapis.com';
mkdirSync(join(dir, 'answers'), { recursive: true });
const log = join(dir, 'attempts.jsonl');

const attempts = () =>
  existsSync(log) ? readFileSync(log, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l)) : [];
const record = (entry) => appendFileSync(log, `${JSON.stringify({ at: new Date().toISOString(), ...entry })}\n`);
const approvals = () => (existsSync(join(dir, 'approvals.json')) ? JSON.parse(readFileSync(join(dir, 'approvals.json'), 'utf8')) : {});

function keyOf(body) {
  const parts = body?.contents?.[0]?.parts ?? [];
  const image = parts.find((p) => p.inline_data)?.inline_data?.data;
  if (!image) return null;
  const text = parts.map((p) => p.text ?? '').join(' ');
  const lang = /\bEnglish\b/.test(text) ? 'en' : /\bJapanese\b/.test(text) ? 'ja' : 'unknown';
  return `${createHash('sha256').update(Buffer.from(image, 'base64')).digest('hex')}:${lang}`;
}

function reply(res, status, body) {
  res.writeHead(status, { 'content-type': 'application/json' });
  res.end(typeof body === 'string' ? body : JSON.stringify(body));
}

createServer(async (req, res) => {
  if (req.method === 'GET' && req.url === '/ledger') {
    const all = attempts();
    const count = (kind) => all.filter((a) => a.kind === kind).length;
    return reply(res, 200, { outbound: count('outbound'), replayed: count('replay'), refused: count('refused'), keys: new Set(all.map((a) => a.key)).size });
  }
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const raw = Buffer.concat(chunks).toString('utf8');
  let body;
  try {
    body = JSON.parse(raw);
  } catch {
    return reply(res, 400, { error: 'not json' });
  }
  const key = keyOf(body);
  if (!key) {
    record({ kind: 'refused', key: null, why: 'no page picture' });
    return reply(res, 429, { error: { message: 'ledger: only page requests are budgeted here' } });
  }
  const saved = join(dir, 'answers', `${key.replace(':', '-')}.json`);
  if (existsSync(saved)) {
    record({ kind: 'replay', key });
    return reply(res, 200, readFileSync(saved, 'utf8'));
  }
  if (process.env.LEDGER_OFFLINE === '1') {
    record({ kind: 'refused', key, why: 'offline' });
    console.log(`refused ${key}: offline, no saved answer`);
    return reply(res, 429, { error: { message: 'ledger: offline, no saved answer for this page' } });
  }
  const sent = attempts().filter((a) => a.kind === 'outbound' && a.key === key).length;
  if (sent >= 1 + (approvals()[key] ?? 0)) {
    record({ kind: 'refused', key, why: 'needs approval' });
    console.log(`refused ${key}: ${sent} attempt(s) already, waiting for approval`);
    return reply(res, 429, { error: { message: 'ledger: another request for this page needs approval' } });
  }
  // Counted before it leaves, so a crash mid-request still counts it.
  record({ kind: 'outbound', key, n: sent + 1 });
  try {
    const up = await fetch(`${UPSTREAM}${req.url}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-goog-api-key': req.headers['x-goog-api-key'] ?? '' },
      body: raw,
    });
    const text = await up.text();
    record({ kind: 'answer', key, status: up.status });
    if (up.status === 200) writeFileSync(saved, text);
    console.log(`${up.status} ${key}`);
    return reply(res, up.status, text);
  } catch (e) {
    record({ kind: 'answer', key, status: 0, why: String(e) });
    return reply(res, 502, { error: { message: `ledger: ${e}` } });
  }
}).listen(Number(port), '127.0.0.1', () => console.log(`gemini ledger on http://127.0.0.1:${port} -> ${dir}`));

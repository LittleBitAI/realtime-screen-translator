// @vitest-environment node
// The development Gemini ledger (scripts/gemini-ledger.mjs), run against a
// local stand-in for Gemini that counts what reaches it.
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

const freePort = () => new Promise((ok) => { const s = createServer().listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => ok(port)); }); });

let dir, upstream, ledger, port, replies, hits;

beforeEach(async () => {
  dir = mkdtempSync(join(tmpdir(), 'ledger-'));
  hits = 0;
  replies = [];
  upstream = createServer((req, res) => {
    req.resume();
    req.on('end', () => {
      hits++;
      const [status, body] = replies.shift() ?? [200, '{"candidates":[]}'];
      res.writeHead(status, { 'content-type': 'application/json' });
      res.end(body);
    });
  });
  await new Promise((ok) => upstream.listen(0, '127.0.0.1', ok));
  port = await freePort();
  ledger = spawn(process.execPath, ['scripts/gemini-ledger.mjs', dir, String(port)], {
    env: { ...process.env, LEDGER_OFFLINE: '', LEDGER_UPSTREAM: `http://127.0.0.1:${upstream.address().port}` },
  });
  await new Promise((ok) => ledger.stdout.on('data', (d) => String(d).includes('gemini ledger on') && ok()));
});

afterEach(async () => {
  ledger.kill();
  await new Promise((ok) => upstream.close(ok));
  rmSync(dir, { recursive: true, force: true });
});

// A page request: the picture, the instruction naming its language on the
// first line, and the chapter's earlier lines after it.
const ask = async (image, lang, context = '') => {
  const text = `This is one manga page. Find every balloon that holds ${lang} dialogue.${context}`;
  const body = { contents: [{ parts: [{ inline_data: { data: Buffer.from(image).toString('base64') } }, { text }] }] };
  const r = await fetch(`http://127.0.0.1:${port}/v1/models/m:generateContent`, { method: 'POST', body: JSON.stringify(body) });
  return [r.status, await r.text()];
};
const keyOf = (image, lang) => `${createHash('sha256').update(Buffer.from(image)).digest('hex')}:${lang}`;

describe('gemini ledger', () => {
  it('keys a page by the language its instruction names, whatever its earlier lines quote', async () => {
    const forms = [
      ['page-a', 'Japanese', '', 'ja'],
      ['page-b', 'Japanese', '\nA: in English => 영어로', 'ja'],
      ['page-c', 'Japanese', '\nA: English? Japanese? => 영어? 일본어?', 'ja'],
      ['page-d', 'English', '', 'en'],
      ['page-e', 'English', '\nA: Japanese class => 일본어 수업', 'en'],
    ];
    for (const [image, lang, context] of forms) expect((await ask(image, lang, context))[0]).toBe(200);
    expect(readdirSync(join(dir, 'answers')).sort()).toEqual(forms.map(([image, , , want]) => `${keyOf(image, want).replace(':', '-')}.json`).sort());
  });

  it('counts a page\'s failed attempt against its later request with earlier lines', async () => {
    replies.push([500, '{"error":{}}']);
    expect((await ask('page-f', 'Japanese'))[0]).toBe(500);
    expect((await ask('page-f', 'Japanese', '\nA: English => 영어'))[0]).toBe(429);
    expect(hits).toBe(1);
  });

  it('sends an approved attempt past an unreadable saved answer, and replays the readable one after', async () => {
    replies.push([200, '{"candidates":[{"content"'], [200, '{"candidates":[]}']);
    expect(await ask('page-g', 'Japanese')).toEqual([200, '{"candidates":[{"content"']);
    expect(await ask('page-g', 'Japanese')).toEqual([200, '{"candidates":[{"content"']);
    expect(hits).toBe(1);
    writeFileSync(join(dir, 'approvals.json'), JSON.stringify({ [keyOf('page-g', 'ja')]: 1 }));
    expect(await ask('page-g', 'Japanese')).toEqual([200, '{"candidates":[]}']);
    expect(hits).toBe(2);
    expect(await ask('page-g', 'Japanese')).toEqual([200, '{"candidates":[]}']);
    expect(hits).toBe(2);
  });
});

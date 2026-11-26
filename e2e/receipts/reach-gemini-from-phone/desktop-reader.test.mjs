import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { arch, platform, release, version } from 'node:os';
import { resolve } from 'node:path';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { tab, targets } from '../balloon-fit/cdp.mjs';

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const detectorHash = '5fe9e4f576e49d4e7e8b0e029d6d3cdc252abd4694113e1cae120e62c931ea79';
const profile = 'ai.littlebit.rst-reach-proof';
const debugPort = 18886;

async function waitFor(read, label) {
  const deadline = Date.now() + 25000;
  let error;
  do {
    try {
      const value = await read();
      if (value) return value;
    } catch (e) {
      error = e;
    }
    await delay(100);
  } while (Date.now() < deadline);
  throw new Error(`Timed out waiting for ${label}`, { cause: error });
}

const html = `<!doctype html><html lang="en"><meta charset="utf-8"><title>Reader evidence fixture</title>
<style>body{margin:0;background:#e8edf4}header{position:fixed;top:8px;right:8px;z-index:1}img{display:block;width:600px}.swiper-slide{min-height:2400px;position:relative}</style>
<header><button id="next">Next fixture page</button></header>
<main><section class="swiper-slide"><img src="/page/1.jpg" alt="Page 1"></section>
<section class="swiper-slide"></section><section class="swiper-slide"></section></main>
<script>
const slides = [...document.querySelectorAll('.swiper-slide')];
let visible = 0;
document.querySelector('#next').onclick = () => {
  visible++;
  const picture = document.createElement('img');
  picture.alt = 'Page ' + (visible + 1);
  picture.src = '/page/' + (visible + 1) + '.jpg';
  slides[visible].append(picture);
  slides[visible].scrollIntoView();
};
const list = new XMLHttpRequest();
list.open('GET', '/chapter-list.json');
list.addEventListener('load', () => { document.body.dataset.listReady = String(list.status); });
list.send();
</script></html>`;

async function makeFixture() {
  const fixture = readFileSync('src-tauri/tests/fixtures/page.jpg');
  const pages = [1, 2, 3].map(n => Buffer.concat([fixture, Buffer.from([n])]));
  const requests = [];
  let reopened = false;
  const server = createServer(async (req, res) => {
    const send = (status, type, body) => res.writeHead(status, { 'content-type': type }).end(body);
    const picture = /^\/page\/([1-3])\.jpg$/.exec(req.url);
    if (picture) return send(200, 'image/jpeg', pages[Number(picture[1]) - 1]);
    if (req.url === '/chapter') return send(200, 'text/html; charset=utf-8', html);
    if (req.url === '/chapter-list.json') {
      return send(200, 'application/json', JSON.stringify({ chapter: [1, 2, 3].map(n => `${origin}/page/${n}.jpg`) }));
    }
    if (req.method === 'POST' && /^\/v1beta\/models\/[^/]+:generateContent(?:\?|$)/.test(req.url)) {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const body = JSON.parse(Buffer.concat(chunks));
      const bytes = Buffer.from(body.contents[0].parts.find(part => part.inline_data).inline_data.data, 'base64');
      requests.push(pages.findIndex(page => page.equals(bytes)) + 1);
      if (reopened) return send(503, 'application/json', '{}');
      const text = JSON.stringify({ box_2d: [150, 200, 250, 300], balloon_2d: [50, 100, 350, 500], src: 'HELLO', ko: '안녕' }) + ']}';
      return send(200, 'application/json', JSON.stringify({ candidates: [{ content: { parts: [{ text }] } }] }));
    }
    send(404, 'text/plain', 'Fixture route not found.');
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const origin = `http://127.0.0.1:${server.address().port}`;
  return { server, origin, requests, rejectFurtherRequests: () => { reopened = true; }, pageSha256: pages.map(sha256) };
}

const readStatus = `window.__TAURI_INTERNALS__.invoke('get_status', {}).then(s => ({ keySet: s.key_set, requests: s.gemini_requests }))`;
const readView = `(() => ({
  scroll: scrollY, pictureCount: document.images.length,
  runPressed: [...document.querySelectorAll('.rst-bar button')].find(b=>b.textContent==='끝까지 번역')?.getAttribute('aria-pressed'),
  status: document.querySelector('.rst-bar [role=status]')?.textContent ?? '',
  chips: [...document.querySelectorAll('.rst-layer .rst-chip')].map(c=>c.textContent),
  visibleKorean: [...document.querySelectorAll('.swiper-slide')].flatMap((slide, index) =>
    [...slide.querySelectorAll('.rst-layer .rst-text')].filter(t=>{
      const r=t.getBoundingClientRect(); return r.width > 0 && r.height > 0 && r.bottom > 0 && r.top < innerHeight;
    }).map(t=>({page:index+1,text:t.textContent}))), viewport: [innerWidth, innerHeight, devicePixelRatio]
}))()`;

test('Windows Tauri reader translates to the end in place and reopens saved pages without Gemini', { timeout: 130000 }, async () => {
  assert.equal(platform(), 'win32');
  const exe = resolve('target/debug/realtime-screen-translator.exe');
  const output = resolve(process.env.RST_DESKTOP_PROOF_OUT ?? 'artifacts/reach-gemini-desktop');
  const model = process.env.RST_DETECTOR_MODEL;
  assert.ok(model && sha256(readFileSync(model)) === detectorHash, 'A verified local detector is required to avoid a download.');
  assert.ok(!existsSync(resolve(process.env.APPDATA, profile, 'translator.sqlite3')), 'Start with a fresh isolated evidence profile.');
  mkdirSync(output, { recursive: true });
  const fixture = await makeFixture();
  const address = `${fixture.origin}/chapter`;
  const env = { ...process.env, RST_GEMINI_BASE_URL: fixture.origin,
    WEBVIEW2_USER_DATA_FOLDER: resolve(output, 'webview-profile'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort} --disable-background-networking` };
  const log = [];
  let child;
  const open = filter => waitFor(async () => (await targets(debugPort)).find(filter), 'native WebView target').then(tab);
  async function start() {
    assert.ok(!child);
    child = spawn(exe, { env, stdio: ['ignore', 'pipe', 'pipe'] });
    child.stdout.on('data', b => log.push(b.toString()));
    child.stderr.on('data', b => log.push(b.toString()));
    const main = await open(t => /^(https?:\/\/tauri\.localhost|tauri:\/\/localhost)/.test(t.url));
    await waitFor(() => main.run(`Boolean(document.querySelector('#site-url') && window.__TAURI_INTERNALS__?.invoke)`), 'Open screen');
    return main;
  }
  async function stop() {
    if (!child) return;
    if (child.exitCode === null && child.signalCode === null) {
      const exited = once(child, 'exit');
      child.kill();
      await exited;
    }
    child = undefined;
    await waitFor(async () => {
      try { await targets(debugPort); return false; } catch { return true; }
    }, 'closed native WebView');
  }
  async function openChapter(main) {
    await waitFor(() => main.run(`document.querySelector('[data-feature="translation-status"] [role=status]')?.textContent.trim() === '번역 준비 완료'`), 'Open screen ready to translate');
    const field = await main.run(`document.querySelector('#site-url').value`);
    if (field !== address) {
      await main.run(`(() => {
        const input = document.querySelector('#site-url');
        input.focus();
        input.select();
      })()`);
      await main.cdp('Input.insertText', { text: address });
    }
    await waitFor(() => main.run(`document.querySelector('#site-url').value === ${JSON.stringify(address)} &&
      !document.querySelector('[data-feature="open-site"]').disabled`), 'fixture address and enabled Open button');
    await main.run(`document.querySelector('[data-feature="open-site"]').click()`);
    return open(t => t.url === address);
  }
  async function click(target, element) {
    const point = await target.run(`(() => {
      const rect = (${element}).getBoundingClientRect();
      return {x: rect.left + rect.width / 2, y: rect.top + rect.height / 2};
    })()`);
    for (const type of ['mousePressed', 'mouseReleased']) {
      await target.cdp('Input.dispatchMouseEvent', { type, ...point, button: 'left', clickCount: 1 });
    }
  }
  const shown = (reader, page) => waitFor(async () => {
    const v = await reader.run(readView);
    return v.visibleKorean.some(row => row.page === page && row.text === '안녕') && !v.chips.length ? v : false;
  }, `visible Korean on fixture page ${page} with no failure chip`);
  const screenshot = async (reader, name) => {
    const image = await reader.cdp('Page.captureScreenshot', { format: 'png' });
    writeFileSync(resolve(output, name), Buffer.from(image.data, 'base64'));
  };
  const next = async (reader, page) => {
    await click(reader, `document.querySelector('#next')`);
    return shown(reader, page);
  };

  let main;
  let reader;
  try {
    main = await start();
    const browser = await (await fetch(`http://127.0.0.1:${debugPort}/json/version`)).json();
    const firstPid = child.pid;
    assert.deepEqual(await main.run(readStatus), { keySet: false, requests: 0 });
    await main.run(`window.__TAURI_INTERNALS__.invoke('put_settings_gemini_key', {key:'desktop-fixture-placeholder'})`);
    reader = await openChapter(main);
    const beforeRun = await shown(reader, 1);
    await waitFor(() => reader.run(`document.body.dataset.listReady === '200'`), 'site chapter-list response');
    assert.deepEqual(fixture.requests, [1]);
    assert.equal(beforeRun.pictureCount, 1);
    await click(reader, `[...document.querySelectorAll('.rst-bar button')].find(b=>b.textContent==='끝까지 번역')`);
    const afterRun = await waitFor(async () => {
      const v = await reader.run(readView);
      return v.status === '회차 끝까지 번역했습니다.' && v.runPressed === 'false' ? v : false;
    }, 'read-to-end completion');
    assert.equal(afterRun.scroll, beforeRun.scroll);
    assert.equal(afterRun.pictureCount, 1);
    assert.deepEqual(afterRun.chips, []);
    assert.deepEqual(fixture.requests, [1, 2, 3]);
    await screenshot(reader, 'windows-read-to-end.png');
    const laterMounted = await next(reader, 2);
    assert.equal(laterMounted.pictureCount, 2);
    assert.deepEqual(fixture.requests, [1, 2, 3]);
    const firstStatus = await main.run(readStatus);
    assert.deepEqual(firstStatus, { keySet: true, requests: 3 });

    reader.close(); reader = undefined;
    main.close(); main = undefined;
    await stop();
    fixture.rejectFurtherRequests();
    main = await start();
    const secondPid = child.pid;
    assert.notEqual(secondPid, firstPid);
    const beforeRestart = await main.run(readStatus);
    assert.deepEqual(beforeRestart, { keySet: true, requests: 0 });
    reader = await openChapter(main);
    const reopened = [await shown(reader, 1), await next(reader, 2), await next(reader, 3)];
    assert.deepEqual(reopened.map(v => v.pictureCount), [1, 2, 3]);
    assert.ok(reopened.every(v => v.runPressed === 'false'));
    const afterRestart = await main.run(readStatus);
    assert.deepEqual(afterRestart, { keySet: true, requests: 0 });
    assert.deepEqual(fixture.requests, [1, 2, 3]);
    await screenshot(reader, 'windows-reopened.png');

    const results = { sourceCommit: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
      environment: { platform: platform(), architecture: arch(), release: release(), version: version(), webview: browser.Browser, node: process.version },
      build: { executable: 'target/debug/realtime-screen-translator.exe', sha256: sha256(readFileSync(exe)), profile, detectorSha256: detectorHash },
      fixture: { pageSha256: fixture.pageSha256, gemini: 'loopback stand-in' },
      initial: { pid: firstPid, beforeRun, afterRun, laterMounted, status: firstStatus },
      reopened: { pid: secondPid, before: beforeRestart, pages: reopened, after: afterRestart, requestOrder: fixture.requests } };
    writeFileSync(resolve(output, 'windows-desktop.json'), JSON.stringify(results, null, 2) + '\n');
  } finally {
    reader?.close();
    main?.close();
    try { await stop(); } finally {
      fixture.server.close();
      writeFileSync(resolve(output, 'windows-process.log'), log.join(''));
    }
  }
});

// The reader script (src-tauri/src/reader.js of the tree it runs from) in
// headless Edge at Galaxy S21 size (360x800 CSS px, DPR 3), over the three
// chapters' page pictures and the backend's answers for them (the PageOut
// JSON replay_chapters writes). invoke() is answered from those files, so
// no Gemini request is made. Writes a screenshot per page and report.json.
// node edge-walk.mjs <repo> <chapters dir> <replay out dir> <shots dir>
import { createServer } from 'node:http';
import { readFile, readdir, mkdir, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';
import { tab, targets } from './cdp.mjs';

const [repo, chapters, out, shots] = process.argv.slice(2);
const html = (await readFile(`${repo}/chapter.html`, 'utf8')).replace('</body>', `<script>
  const calls = [];
  window.__rstCalls = calls;
  const book = new URLSearchParams(location.search).get('id');
  window.__TAURI_INTERNALS__ = { invoke: async (cmd, a) => {
    calls.push([cmd, a?.picture_id ?? a?.book_id]);
    if (cmd === 'get_chapter') return fetch('/chapter/' + book).then(r => r.json());
    if (cmd === 'get_chapter_image') return fetch('/img/' + book + '/' + a.picture_id).then(r => r.arrayBuffer());
    if (cmd === 'post_chapter_page') return fetch('/out/' + book + '/' + a.picture_id).then(r => r.json());
    return null;
  } };
</script><script src="/reader.js"></script></body>`);
const pad = (n) => String(n).padStart(2, '0');
const server = createServer(async (req, res) => {
  const u = new URL(req.url, 'http://x');
  const [, kind, ch, n] = u.pathname.split('/');
  try {
    if (u.pathname === '/chapter.html') { res.setHeader('content-type', 'text/html; charset=utf-8'); return res.end(html); }
    if (u.pathname === '/reader.js') { res.setHeader('content-type', 'text/javascript'); return res.end(await readFile(`${repo}/src-tauri/src/reader.js`)); }
    if (kind === 'chapter') {
      const files = (await readdir(`${out}/ch${ch}`)).filter((f) => f.endsWith('.json')).sort();
      const pages = await Promise.all(files.map(async (f, i) => { const p = JSON.parse(await readFile(`${out}/ch${ch}/${f}`, 'utf8')); return { id: i + 1, width: p.width, height: p.height }; }));
      res.setHeader('content-type', 'application/json');
      return res.end(JSON.stringify({ title: `ch${ch}`, pages }));
    }
    if (kind === 'img') return res.end(await readFile(`${chapters}/ch${ch}/${pad(n)}.jpg`));
    if (kind === 'out') { res.setHeader('content-type', 'application/json'); return res.end(await readFile(`${out}/ch${ch}/${pad(n)}.json`)); }
  } catch (e) { res.statusCode = 404; return res.end(String(e)); }
  res.statusCode = 404; res.end();
});
await new Promise((ok) => server.listen(18921, '127.0.0.1', ok));
const edge = spawn('C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', ['--headless=new', '--remote-debugging-port=18922', `--user-data-dir=${shots}/profile`, '--no-first-run', 'about:blank'], { stdio: 'ignore' });
let t;
// The blank page it opened: a newer Edge may list a page of its own first.
for (let i = 0; i < 240 && !t; i++) { await delay(250); t = (await targets(18922).catch(() => [])).find((p) => p.url === 'about:blank'); }
const s = await tab(t);
await s.cdp('Emulation.setDeviceMetricsOverride', { width: 360, height: 800, deviceScaleFactor: 3, mobile: true });
const report = {};
for (const ch of ['256', '257', '258']) {
  await mkdir(`${shots}/ch${ch}`, { recursive: true });
  await s.cdp('Page.navigate', { url: `http://127.0.0.1:18921/chapter.html?id=${ch}` });
  await delay(1500);
  const total = await s.run('document.querySelectorAll("main img").length');
  const seen = [];
  for (let i = 0; i < total; i++) {
    await s.run(`document.querySelectorAll('main img')[${i}].scrollIntoView({ block: 'start' })`);
    let state;
    for (let k = 0; k < 60; k++) {
      await delay(250);
      state = await s.run(`(() => { const img = document.querySelectorAll('main img')[${i}]; const layer = img.parentElement.querySelector('.rst-layer');
        const boxes = layer ? [...layer.querySelectorAll('.rst-box')] : [];
        const texts = layer ? [...layer.querySelectorAll('.rst-text')] : [];
        const patches = layer ? [...layer.querySelectorAll('canvas.rst-patch')] : [];
        // A text's lines as drawn, and the size drawn against the page's
        // least size (readable, or its own lettering where that is smaller).
        const range = document.createRange();
        const extent = (t) => { range.selectNodeContents(t); return range.getBoundingClientRect(); };
        // The size set, times any scale the text is drawn at.
        const drawn = (t) => { const s = getComputedStyle(t); return parseFloat(s.fontSize) * (s.transform === 'none' ? 1 : new DOMMatrix(s.transform).a); };
        const least = (t) => Math.min(7, Number(t.dataset.least) * img.offsetWidth || 7);
        return { loaded: img.complete && img.naturalWidth > 0, boxes: boxes.length, texts: texts.length,
          // Cut off: lines past a text area that hides its overflow.
          clipped: texts.filter((t) => getComputedStyle(t).overflow !== 'visible' && (t.scrollHeight > t.clientHeight + 1 || t.scrollWidth > t.clientWidth + 1)).length,
          belowLeast: texts.filter((t) => drawn(t) < least(t) - 0.05).length,
          // Lines shown past their cover's box: a white cover's (a balloon),
          // or toned letters' on the art, where the lines' white rim sits on it.
          pastCover: texts.filter((t) => { const [l, b] = [extent(t), t.parentElement.getBoundingClientRect()]; return l.width > 0 && (l.left < b.left - 1 || l.right > b.right + 1 || l.top < b.top - 1 || l.bottom > b.bottom + 1); })
            .map((t) => ({ ko: t.textContent, white: !!t.parentElement.querySelector('.rst-fill') })),
          // Painted: the decoded patch drawn, some pixel of it opaque. The
          // canvas has its size before its picture is decoded and drawn.
          patches: patches.length, unpainted: patches.filter((c) => !c.width || !c.height || !c.getContext('2d').getImageData(0, 0, c.width, c.height).data.some((v, j) => j % 4 === 3 && v > 0)).length,
          minPx: texts.length ? Math.min(...texts.map(drawn)) : null };
      })()`);
      if (state.loaded && state.boxes > 0 && state.patches === 0 || state.loaded && state.boxes > 0 && state.unpainted === 0) break;
    }
    await delay(300);
    const shot = await s.cdp('Page.captureScreenshot', { format: 'png' });
    await writeFile(`${shots}/ch${ch}/${pad(i + 1)}.png`, Buffer.from(shot.data, 'base64'));
    seen.push({ page: i + 1, ...state });
  }
  const sent = (await s.run('window.__rstCalls')).filter(([c]) => c === 'post_chapter_page').map(([, id]) => id);
  report[`ch${ch}`] = { total, drawn: seen.filter((p) => p.loaded && p.boxes > 0).length, inOrder: sent.every((v, i) => i === 0 || v > sent[i - 1]), sent: sent.length,
    clipped: seen.reduce((a, p) => a + p.clipped, 0), belowLeast: seen.reduce((a, p) => a + p.belowLeast, 0), pastWhite: seen.reduce((a, p) => a + p.pastCover.filter((c) => c.white).length, 0), pastTone: seen.reduce((a, p) => a + p.pastCover.filter((c) => !c.white).length, 0), unpainted: seen.reduce((a, p) => a + p.unpainted, 0), patches: seen.reduce((a, p) => a + p.patches, 0),
    minPx: Math.min(...seen.filter((p) => p.minPx).map((p) => p.minPx)).toFixed(1), pages: seen };
  console.log(`ch${ch}`, JSON.stringify({ ...report[`ch${ch}`], pages: undefined }));
}
await writeFile(`${shots}/report.json`, JSON.stringify(report, null, 1));
console.log('browser', await s.run('navigator.userAgent'));
s.close();
edge.kill();
server.close();

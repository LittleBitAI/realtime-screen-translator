// Galaxy S21+ read-through: opens each chapter address in the installed reader
// (the command the address bar uses), turns MangaFire's page slider one page
// at a time, waits for that page's shapes, and captures the screen with the
// translation on and off. Usage: node phone-walk.mjs <out dir> <url>...
// With WALK_PORT set, the installed Windows app instead: its WebView2's
// debugging port, each capture the page as the window shows it.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { tab, targets } from './cdp.mjs';

const adb = 'C:/Users/dasdk/AppData/Local/Android/Sdk/platform-tools/adb.exe';
const port = Number(process.env.WALK_PORT ?? 18847);
const desktop = 'WALK_PORT' in process.env;
const [out, ...urls] = process.argv.slice(2);
let site;
const shot = async (file) => writeFileSync(file, desktop
  ? Buffer.from((await site.cdp('Page.captureScreenshot', { format: 'png' })).data, 'base64')
  : execFileSync(adb, ['exec-out', 'screencap', '-p'], { maxBuffer: 64 << 20 }));
async function wait(read, name, tries = 240) {
  let problem;
  for (let i = 0; i < tries; i++) {
    try { const r = await read(); if (r) return r; } catch (e) { problem = e; }
    await delay(250);
  }
  throw Error(name, { cause: problem });
}
const page = (filter) => wait(async () => (await targets(port)).find(filter), 'page');
// The app's own screens: a site page may only send pages and close.
async function home() {
  const now = await page(() => true);
  if (!now.url.includes('tauri.localhost')) {
    const site = await tab(now);
    await site.run(`window.__TAURI_INTERNALS__.invoke('post_reader_close', {})`).catch(() => {});
    site.close();
  }
  // The app's screen comes back by loading again: wait for its bridge.
  const app = await tab(await page((p) => p.url.includes('tauri.localhost')));
  await wait(() => app.run(`!!window.__TAURI_INTERNALS__?.invoke`), 'app bridge');
  return app;
}
const status = async () => {
  const app = await home();
  try { return await app.run(`window.__TAURI_INTERNALS__.invoke('get_status', {}).then(s => s.gemini_requests)`); } finally { app.close(); }
};

const report = { gemini_requests_before: await status(), chapters: [] };
for (const url of urls) {
  const app = await home();
  await app.run(`window.__TAURI_INTERNALS__.invoke('post_reader_open', { url: ${JSON.stringify(url)} })`);
  app.close();
  await delay(3000);
  site = await tab(await page((p) => p.url === url));
  const slides = await wait(() => site.run(`document.querySelector('.swiper')?.swiper?.slides?.length`), 'slider');
  const chapter = url.split('/').pop();
  const dir = `${out}/${chapter}`;
  mkdirSync(dir, { recursive: true });
  const pages = [];
  for (let k = 0; k < slides; k++) {
    await site.run(`document.querySelector('.swiper').swiper.slideTo(${k}, 0)`);
    // The page pictures on this slide (one on the phone, two side by side on
    // the desktop's wider page, none on the chapter's closing slide), each
    // with its layer: shapes drawn, or a chip saying why not. A slide's
    // pictures may be put in a moment after it is shown.
    const shown = Date.now();
    const state = await wait(() => site.run(`(() => { const s = document.querySelector('.swiper').swiper.slides[${k}];
      const imgs = [...s.querySelectorAll('img.reader-img')];
      if (!imgs.length) return null;
      const out = [];
      for (const i of imgs) {
        // A picture the site failed to send is asked for again, as a reload would.
        if (i.complete && !i.naturalWidth && i.src) { const u = i.src; i.removeAttribute('src'); i.src = u; return null; }
        if (!i.complete || !i.naturalWidth) return null;
        const layer = i.nextElementSibling?.classList.contains('rst-layer') ? i.nextElementSibling : null;
        const chip = layer?.querySelector('.rst-chip')?.textContent;
        if (chip) { if (chip.startsWith('번역 중')) return null; out.push({ picture: true, chip }); continue; }
        if (!layer || !layer.dataset.at) return null;
        out.push({ picture: true, boxes: layer.querySelectorAll('.rst-box').length, w: i.naturalWidth, h: i.naturalHeight });
      }
      return out; })()`).then((r) => r ?? (Date.now() - shown > 6000 && site.run(`document.querySelector('.swiper').swiper.slides[${k}].querySelectorAll('img.reader-img').length`).then((n) => (n ? null : [])))), `slide ${k + 1}`, 480);
    if (!state.length) { console.log(chapter, 'slide', k + 1, 'no page picture'); continue; }
    await delay(500);
    // The sizes as captured: read after the layer's last refit, not when it first appears.
    // Each text: the size drawn (set size times any scale), the page's least
    // size there (readable, or its own lettering where smaller), whether
    // its lines are cut off, and whether they show past a white cover's
    // box (a balloon) or toned letters' box (on the art).
    const texts = await site.run(`[...document.querySelector('.swiper').swiper.slides[${k}].querySelectorAll('img.reader-img')].map((i) => [...(i.nextElementSibling?.querySelectorAll('.rst-text') ?? [])].map((t) => {
      const s = getComputedStyle(t); const r = document.createRange(); r.selectNodeContents(t);
      const [l, b] = [r.getBoundingClientRect(), t.parentElement.getBoundingClientRect()];
      const past = l.width > 0 && (l.left < b.left - 1 || l.right > b.right + 1 || l.top < b.top - 1 || l.bottom > b.bottom + 1);
      // The lines as drawn against their own text area as laid out (its
      // scale and move taken off for the moment of reading it).
      const tf = t.style.transform; t.style.transform = 'none'; const a = t.getBoundingClientRect(); t.style.transform = tf;
      const white = !!t.parentElement.querySelector('.rst-fill');
      return { px: +(parseFloat(s.fontSize) * (s.transform === 'none' ? 1 : new DOMMatrix(s.transform).a)).toFixed(2),
        least: +Math.min(7, Number(t.dataset.least) * i.offsetWidth || 7).toFixed(2),
        cut: s.overflow !== 'visible' && (t.scrollHeight > t.clientHeight + 1 || t.scrollWidth > t.clientWidth + 1),
        past: past ? (white ? 'white' : 'tone') : null,
        white, past_area: l.width > 0 && (l.left < a.left - 1 || l.right > a.right + 1 || l.top < a.top - 1 || l.bottom > a.bottom + 1) };
    }))`);
    // Each tone cover: drawn at the share of its box set, or bent by the
    // site's own sheet (a canvas capped at its box's width) off the letters.
    const patches = await site.run(`[...document.querySelector('.swiper').swiper.slides[${k}].querySelectorAll('img.reader-img')].map((i) => [...(i.nextElementSibling?.querySelectorAll('.rst-patch') ?? [])].map((p) => {
      const [q, b] = [p.getBoundingClientRect(), p.parentElement.getBoundingClientRect()];
      return Math.abs(q.width / b.width - parseFloat(p.style.width) / 100) > 0.01 || Math.abs(q.height / b.height - parseFloat(p.style.height) / 100) > 0.01;
    }))`);
    state.forEach((p, j) => { if (!p.chip) [p.texts, p.patches, p.patches_off] = [texts[j], patches[j].length, patches[j].filter(Boolean).length]; });
    const name = `${dir}/${String(pages.length + 1).padStart(2, '0')}`;
    // Each capture after the page has drawn twice since the switch: the
    // screen grab may hold the frame from before it (ch257 p6, one pair
    // the same with nine shapes). A pair still the same is recorded.
    const drawn = () => site.run(`new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(() => ok(true))))`);
    const sum = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
    const boxes = state.reduce((a, p) => a + (p.boxes ?? 0), 0);
    await shot(`${name}-after.png`);
    await site.run(`document.documentElement.classList.add('rst-off')`);
    let same = true;
    for (let tries = 0; tries < 5 && same; tries++) {
      await drawn();
      await delay(200 * (tries + 1));
      await shot(`${name}-before.png`);
      same = boxes > 0 && sum(`${name}-before.png`) === sum(`${name}-after.png`);
    }
    await site.run(`document.documentElement.classList.remove('rst-off')`);
    await drawn();
    for (const p of state) {
      pages.push({ page: pages.length + 1, slide: k + 1, ...p, same_capture: same });
      console.log(chapter, 'page', pages.length, JSON.stringify({ ...p, texts: undefined, min_font: Math.min(...(p.texts ?? []).map((t) => t.px)) }));
    }
  }
  site.close();
  const all = pages.flatMap((p) => p.texts ?? []);
  report.chapters.push({ url, slides, pages: pages.length, chips: pages.filter((p) => p.chip).length, same_captures: pages.filter((p) => p.same_capture).length,
    below_least: all.filter((t) => t.px < t.least - 0.05).length, cut: all.filter((t) => t.cut).length,
    past_white: all.filter((t) => t.past === 'white').length, past_tone: all.filter((t) => t.past === 'tone').length,
    white_past_area: all.filter((t) => t.white && t.past_area).length,
    patches: pages.reduce((a, p) => a + (p.patches ?? 0), 0), patches_off: pages.reduce((a, p) => a + (p.patches_off ?? 0), 0), detail: pages });
}
report.gemini_requests_after = (await wait(async () => ({ n: await status() }), 'status', 40)).n;
writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 1));
console.log('app gemini_requests', report.gemini_requests_before, '->', report.gemini_requests_after);

// Galaxy S21+ read-through: opens each chapter address in the installed reader
// (the command the address bar uses), turns MangaFire's page slider one page
// at a time, waits for that page's shapes, and captures the screen with the
// translation on and off. Usage: node balloonfit-phone.mjs <out dir> <url>...
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { tab, targets } from './cdp.mjs';

const adb = 'C:/Users/dasdk/AppData/Local/Android/Sdk/platform-tools/adb.exe';
const port = 18847;
const [out, ...urls] = process.argv.slice(2);
const shot = (file) => writeFileSync(file, execFileSync(adb, ['exec-out', 'screencap', '-p'], { maxBuffer: 64 << 20 }));
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
  const site = await tab(await page((p) => p.url === url));
  const slides = await wait(() => site.run(`document.querySelector('.swiper')?.swiper?.slides?.length`), 'slider');
  const chapter = url.split('/').pop();
  const dir = `${out}/${chapter}`;
  mkdirSync(dir, { recursive: true });
  const pages = [];
  for (let k = 0; k < slides; k++) {
    await site.run(`document.querySelector('.swiper').swiper.slideTo(${k}, 0)`);
    // The page picture on this slide (none on the chapter's closing slide),
    // and its layer: shapes drawn, or a chip saying why not.
    const state = await wait(() => site.run(`(() => { const s = document.querySelector('.swiper').swiper.slides[${k}];
      const i = s.querySelector('img.reader-img'); if (!i) return { picture: false };
      // A picture the site failed to send is asked for again, as a reload would.
      if (i.complete && !i.naturalWidth && i.src) { const u = i.src; i.removeAttribute('src'); i.src = u; return null; }
      if (!i.complete || !i.naturalWidth) return null;
      const layer = i.nextElementSibling?.classList.contains('rst-layer') ? i.nextElementSibling : null;
      const chip = layer?.querySelector('.rst-chip')?.textContent;
      if (chip) return chip.startsWith('번역 중') ? null : { picture: true, chip };
      const boxes = layer?.querySelectorAll('.rst-box').length ?? 0;
      return layer && layer.dataset.at ? { picture: true, boxes, w: i.naturalWidth, h: i.naturalHeight, texts: [...layer.querySelectorAll('.rst-text')].map(t => parseFloat(t.style.fontSize)) } : null; })()`), `slide ${k + 1}`, 480);
    if (!state.picture) { console.log(chapter, 'slide', k + 1, 'no page picture'); continue; }
    await delay(500);
    // The sizes as captured: read after the layer's last refit, not when it first appears.
    if (state.texts) state.texts = await site.run(`[...document.querySelector('.swiper').swiper.slides[${k}].querySelectorAll('.rst-layer .rst-text')].map(t => parseFloat(t.style.fontSize))`);
    const name = `${dir}/${String(pages.length + 1).padStart(2, '0')}`;
    shot(`${name}-after.png`);
    await site.run(`document.documentElement.classList.add('rst-off')`);
    await delay(200);
    shot(`${name}-before.png`);
    await site.run(`document.documentElement.classList.remove('rst-off')`);
    pages.push({ page: pages.length + 1, ...state });
    console.log(chapter, 'page', pages.length, JSON.stringify({ ...state, texts: undefined, min_font: Math.min(...(state.texts ?? [0])) }));
  }
  site.close();
  report.chapters.push({ url, slides, pages: pages.length, chips: pages.filter((p) => p.chip).length, detail: pages });
}
report.gemini_requests_after = (await wait(async () => ({ n: await status() }), 'status', 40)).n;
writeFileSync(`${out}/report.json`, JSON.stringify(report, null, 1));
console.log('app gemini_requests', report.gemini_requests_before, '->', report.gemini_requests_after);

// The reader's page script (src-tauri/src/reader.js) sending a site's pages
// to the app. Apart from reader.test.js: a reader started there keeps
// watching the page and would send these pages too.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import script from '../../src-tauri/src/reader.js?raw';

function pagePicture(src) {
  const img = document.createElement('img');
  const own = { complete: true, naturalWidth: 600, naturalHeight: 800, offsetWidth: 300, offsetHeight: 400, currentSrc: src, src };
  for (const [k, v] of Object.entries(own)) Object.defineProperty(img, k, { value: v });
  document.body.appendChild(img);
  return img;
}

beforeEach(() => {
  vi.stubGlobal('setInterval', vi.fn());
  vi.stubGlobal('requestAnimationFrame', (f) => setTimeout(f, 0));
  Range.prototype.getBoundingClientRect ??= () => new DOMRect();
  Range.prototype.getClientRects ??= () => [];
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('reader page script, pages in order', () => {
  // The app has a page wait for the earlier ones it is reading, so the
  // later page's request carries their lines; it can only wait for those
  // it has been sent.
  it('sends pages in reading order even when a later page\'s picture is fetched sooner', async () => {
    const picture = { ok: true, blob: async () => new Blob(['x'], { type: 'image/png' }) };
    vi.stubGlobal('fetch', vi.fn(async (src) => {
      if (src.endsWith('/1.jpg')) await new Promise((ok) => setTimeout(ok, 50));
      return picture;
    }));
    const answer = { width: 600, height: 800, saved: false, boxes: [], save_error: null };
    const invoke = vi.fn().mockResolvedValue(answer);
    window.__TAURI_INTERNALS__ = { invoke };
    pagePicture('https://cdn.example/1.jpg');
    pagePicture('https://cdn.example/2.jpg');

    new Function(script)();

    const sent = () => invoke.mock.calls.filter(([c]) => c === 'post_reader_page').map(([, a]) => [a.src, a.index]);
    await vi.waitFor(() => expect(sent()).toHaveLength(2));
    expect(sent()).toEqual([
      ['https://cdn.example/1.jpg', 0],
      ['https://cdn.example/2.jpg', 1],
    ]);
  });
});

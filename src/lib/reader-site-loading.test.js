// The reader's page script (src-tauri/src/reader.js) on a site whose page
// pictures finish loading out of order. One reader per file: a script
// started keeps watching the document.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import script from '../../src-tauri/src/reader.js?raw';

// state: the picture's loading state, read live.
function pagePicture(src, state = {}) {
  const img = document.createElement('img');
  const own = { complete: true, naturalWidth: 600, naturalHeight: 800, offsetWidth: 300, offsetHeight: 400, currentSrc: src, src };
  for (const k of Object.keys(own)) Object.defineProperty(img, k, { get: () => state[k] ?? own[k] });
  document.body.appendChild(img);
}

const settle = () => new Promise((ok) => setTimeout(ok, 50));

beforeEach(() => {
  vi.stubGlobal('requestAnimationFrame', (f) => setTimeout(f, 0));
  Range.prototype.getBoundingClientRect ??= () => new DOMRect();
  Range.prototype.getClientRects ??= () => [];
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('reader page script, site pictures still loading', () => {
  // A later page's request carries the lines of the earlier pages the app
  // has been sent, so it waits for an earlier picture still loading; one
  // that fails (loaded, no size) is no page and holds nothing.
  it('sends a page only after every earlier page picture has loaded or failed', async () => {
    const picture = { ok: true, blob: async () => new Blob(['x'], { type: 'image/png' }) };
    vi.stubGlobal('fetch', vi.fn(async () => picture));
    let scan;
    vi.stubGlobal('setInterval', (f) => { scan = f; });
    const answer = { width: 600, height: 800, saved: false, boxes: [], save_error: null };
    const invoke = vi.fn().mockResolvedValue(answer);
    window.__TAURI_INTERNALS__ = { invoke };
    const loading = () => ({ complete: false, naturalWidth: 0, naturalHeight: 0 });
    const first = loading();
    const second = loading();
    pagePicture('https://cdn.example/1.jpg', first);
    pagePicture('https://cdn.example/2.jpg', second);
    pagePicture('https://cdn.example/3.jpg');

    new Function(script)();
    const sent = () => invoke.mock.calls.filter(([c]) => c === 'post_reader_page').map(([, a]) => a.src.split('/').pop());

    await settle();
    expect(sent()).toEqual([]);

    Object.assign(first, { complete: true, naturalWidth: 600, naturalHeight: 800 });
    scan();
    await vi.waitFor(() => expect(sent()).toEqual(['1.jpg']));
    await settle();
    scan();
    await settle();
    expect(sent()).toEqual(['1.jpg']);

    Object.assign(second, { complete: true });
    scan();
    await vi.waitFor(() => expect(sent()).toEqual(['1.jpg', '3.jpg']));
  });
});

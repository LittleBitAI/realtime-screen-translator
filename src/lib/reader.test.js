// The reader's page script (src-tauri/src/reader.js) on a page with one manga
// picture, the app's commands stood in for by `invoke`.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import script from '../../src-tauri/src/reader.js?raw';

function pagePicture(src) {
  const img = document.createElement('img');
  const own = { complete: true, naturalWidth: 600, naturalHeight: 800, offsetWidth: 300, offsetHeight: 400, currentSrc: src, src };
  for (const [k, v] of Object.entries(own)) Object.defineProperty(img, k, { value: v });
  document.body.appendChild(img);
  return img;
}

const page = (save_error) => ({
  width: 600,
  height: 800,
  saved: false,
  boxes: [{ x: 60, y: 80, w: 120, h: 60, radius: 6, font_px: 20, ko: '안녕', outline: [], text: [66, 86, 108, 48] }],
  save_error,
});

beforeEach(() => {
  vi.stubGlobal('setInterval', vi.fn());
  vi.stubGlobal('requestAnimationFrame', (f) => setTimeout(f, 0));
  // jsdom lays nothing out: the lines' extent is empty, as no spill.
  Range.prototype.getBoundingClientRect ??= () => new DOMRect();
});

afterEach(() => {
  document.head.replaceChildren();
  document.body.replaceChildren();
  history.replaceState(null, '', '/');
  delete window.__rstReader;
  delete window.__TAURI_INTERNALS__;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('reader page script', () => {
  it('shows a page it could not keep in the library, says so, and saves it on a tap', async () => {
    // The site does not hand over the bytes: the app fetches them itself.
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('cors'); }));
    const invoke = vi.fn().mockResolvedValue(null).mockResolvedValueOnce(page('storage_full')).mockResolvedValueOnce(page(null));
    window.__TAURI_INTERNALS__ = { invoke };
    pagePicture('https://cdn.example/1.jpg');

    new Function(script)();

    const chip = await vi.waitFor(() => {
      const c = document.querySelector('.rst-chip');
      expect(c?.textContent).toBe('번역본에 저장 못 함 · 눌러서 다시 저장');
      return c;
    });
    expect(document.querySelector('.rst-box').textContent).toBe('안녕');
    expect(document.querySelector('.rst-bar [role=status]').textContent).toContain('저장 공간');
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke.mock.calls[0][1]).toMatchObject({ src: 'https://cdn.example/1.jpg', image: null });

    chip.click();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(document.querySelector('.rst-chip')).toBeNull());
    expect(document.querySelector('.rst-box').textContent).toBe('안녕');
    expect(document.querySelector('.rst-bar [role=status]').hidden).toBe(true);
    [...document.querySelectorAll('.rst-bar button')].find((button) => button.textContent === '앱으로 돌아가기').click();
    expect(invoke).toHaveBeenLastCalledWith('post_reader_close', {});
  });

  it('shapes each cover to its balloon, sets the Korean in its text area, and draws nothing for a line kept without a shape', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('cors'); }));
    const balloon = { x: 100, y: 200, w: 200, h: 100, radius: 0, font_px: 20, ko: '말풍선',
      outline: [[300, 200], [300, 300], [100, 300], [100, 250], [150, 250], [150, 200]], text: [150, 220, 100, 60] };
    const kept = { x: 400, y: 400, w: 0, h: 0, radius: 0, font_px: 0, ko: '요들레이히!', outline: [], text: [400, 400, 0, 0] };
    const invoke = vi.fn().mockResolvedValue(null).mockResolvedValueOnce({ ...page(null), boxes: [balloon, kept] });
    window.__TAURI_INTERNALS__ = { invoke };
    pagePicture('https://cdn.example/2.jpg');

    new Function(script)();

    const box = await vi.waitFor(() => {
      const b = document.querySelector('.rst-box');
      expect(b).not.toBeNull();
      return b;
    });
    expect([...document.querySelectorAll('.rst-box')].map((b) => b.textContent)).toEqual(['말풍선']);
    expect(box.style.clipPath).toBe('polygon(100% 0%, 100% 100%, 0% 100%, 0% 50%, 25% 50%, 25% 0%)');
    const text = box.querySelector('.rst-text');
    expect([text.style.left, text.style.top, text.style.width, text.style.height]).toEqual(['25%', '20%', '50%', '60%']);
    expect(text.textContent).toBe('말풍선');
  });

  it('sets a line lettered in two balloons a share in each', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('cors'); }));
    const two = { x: 100, y: 200, w: 400, h: 100, radius: 0, font_px: 20, ko: '응. 알았어.', outline: [[100, 200], [500, 200], [500, 300], [100, 300]], text: [400, 220, 80, 60],
      parts: [{ text: [400, 220, 80, 60], font_px: 20, ko: '응.' }, { text: [120, 220, 80, 60], font_px: 16, ko: '알았어.' }] };
    const invoke = vi.fn().mockResolvedValue(null).mockResolvedValueOnce({ ...page(null), boxes: [two] });
    window.__TAURI_INTERNALS__ = { invoke };
    pagePicture('https://cdn.example/3.jpg');

    new Function(script)();

    const texts = await vi.waitFor(() => {
      const t = [...document.querySelectorAll('.rst-box .rst-text')];
      expect(t).toHaveLength(2);
      return t;
    });
    expect(texts.map((t) => [t.textContent, t.style.left, t.style.width])).toEqual([['응.', '75%', '20%'], ['알았어.', '5%', '20%']]);
  });

  it('loads and translates a chapter picture only as it approaches the view', async () => {
    history.replaceState(null, '', '/chapter.html?id=4');
    document.head.innerHTML = '<meta name="rst-app"><meta name="rst-chapter">';
    document.body.innerHTML = '<h1></h1><main></main>';
    vi.stubGlobal('URL', class extends URL {
      static createObjectURL() { return 'blob:fixture-page'; }
      static revokeObjectURL() {}
    });
    let approaching = false;
    const img = HTMLImageElement.prototype;
    vi.spyOn(img, 'naturalWidth', 'get').mockImplementation(function () {
      return this.hasAttribute('src') ? 600 : 0;
    });
    vi.spyOn(img, 'complete', 'get').mockReturnValue(true);
    vi.spyOn(img, 'getBoundingClientRect').mockImplementation(() => ({
      top: approaching ? 1200 : 3000, bottom: approaching ? 2000 : 3800, left: 0, right: 600,
    }));
    const invoke = vi.fn().mockImplementation(async (command) => ({
      get_chapter: { title: 'Fixture', pages: [{ id: 17, width: 600, height: 800 }] },
      get_chapter_image: new Uint8Array([1, 2, 3]).buffer,
      post_chapter_page: page(null),
    })[command]);
    window.__TAURI_INTERNALS__ = { invoke };
    new Function(script)();

    await vi.waitFor(() => expect(document.querySelector('main img')).not.toBeNull());
    expect(invoke.mock.calls).toEqual([['get_chapter', { book_id: 4 }]]);
    approaching = true;
    window.dispatchEvent(new Event('scroll'));
    await vi.waitFor(() => expect(document.querySelector('.rst-box')?.textContent).toBe('안녕'));
    expect(invoke.mock.calls.slice(1)).toEqual([
      ['get_chapter_image', { picture_id: 17 }],
      ['post_chapter_page', { picture_id: 17 }],
    ]);
    expect(document.querySelector('.rst-box').style.left).toBe('10%');
  });
});

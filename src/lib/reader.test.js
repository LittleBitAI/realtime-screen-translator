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
  Range.prototype.getClientRects ??= () => [];
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

  it('shapes each cover to its balloon or the art tone, sets the Korean in its text area, and draws nothing for a line kept without a shape', async () => {
    // The tone is painted from its own bytes, no image address loaded, so a
    // site whose image policy refuses `data:` addresses still shows it.
    const bitmap = { width: 100, height: 50 };
    const decode = vi.fn(async () => bitmap);
    vi.stubGlobal('createImageBitmap', decode);
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({ drawImage });
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('cors'); }));
    const balloon = { x: 100, y: 200, w: 200, h: 100, radius: 0, font_px: 20, ko: '말풍선',
      outline: [[300, 200], [300, 300], [100, 300], [100, 250], [150, 250], [150, 200]], text: [150, 220, 100, 60] };
    const kept = { x: 400, y: 400, w: 0, h: 0, radius: 0, font_px: 0, ko: '요들레이히!', outline: [], text: [400, 400, 0, 0] };
    const art = { x: 100, y: 400, w: 200, h: 100, radius: 0, font_px: 20, ko: '야호', outline: [], text: [100, 400, 200, 100], patch: { at: [150, 425, 100, 50], png: 'AAAA' } };
    const invoke = vi.fn().mockResolvedValue(null).mockResolvedValueOnce({ ...page(null), boxes: [balloon, kept, art] });
    window.__TAURI_INTERNALS__ = { invoke };
    pagePicture('https://cdn.example/2.jpg');

    new Function(script)();

    const box = await vi.waitFor(() => {
      const b = document.querySelector('.rst-box');
      expect(b).not.toBeNull();
      return b;
    });
    expect([...document.querySelectorAll('.rst-box')].map((b) => b.textContent)).toEqual(['말풍선', '야호']);
    // The white is cut to the outline; the Korean over it is not.
    expect(box.style.clipPath).toBe('');
    expect(box.querySelector('.rst-fill').style.clipPath).toBe('polygon(100% 0%, 100% 100%, 0% 100%, 0% 50%, 25% 50%, 25% 0%)');
    const text = box.querySelector('.rst-text');
    expect([text.style.left, text.style.top, text.style.width, text.style.height]).toEqual(['25%', '20%', '50%', '60%']);
    expect(text.textContent).toBe('말풍선');
    // Lettering with no balloon: the art's tone over it, and no white.
    const tone = document.querySelectorAll('.rst-box')[1];
    expect(tone.querySelector('.rst-fill')).toBeNull();
    const patch = tone.querySelector('.rst-patch');
    expect([patch.style.left, patch.style.top, patch.style.width, patch.style.height]).toEqual(['25%', '25%', '50%', '50%']);
    expect([patch.tagName, patch.width, patch.height]).toEqual(['CANVAS', 100, 50]);
    await vi.waitFor(() => expect(drawImage).toHaveBeenCalledWith(bitmap, 0, 0));
    const blob = decode.mock.calls[0][0];
    expect([blob.type, [...new Uint8Array(await blob.arrayBuffer())]]).toEqual(['image/png', [0, 0, 0]]);
    expect([...document.querySelectorAll('.rst-layer *')].some((e) => (e.getAttribute('style') ?? '').includes('url('))).toBe(false);
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

  it('keeps a dash on the line of the word before it', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('cors'); }));
    const invoke = vi.fn().mockResolvedValue(null).mockResolvedValueOnce({ ...page(null), boxes: [{ ...page(null).boxes[0], ko: '아—?! 13일의...?!' }] });
    window.__TAURI_INTERNALS__ = { invoke };
    pagePicture('https://cdn.example/4.jpg');

    new Function(script)();

    const text = await vi.waitFor(() => {
      const t = document.querySelector('.rst-text');
      expect(t).not.toBeNull();
      return t;
    });
    expect(text.textContent).toBe('아⁠—?! 13일의...?!');
  });

  // The least size is readable (7 px), or, on a page shown so small that
  // its own lettering is smaller (a spread across a phone), the size the
  // original has there: the layout's size at 0.7. The picture is shown at
  // half its width, so a 20 px layout size is 10 px on screen (least 7) and
  // an 8 px one 4 px (least 2.8).
  it('scales Korean too long for its text area at the least size down whole into that area, never past it', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('cors'); }));
    // A rectangle cover, and a balloon with its outline holding a line in
    // two shares: each a 16 x 10 text area.
    const long = '가'.repeat(100);
    const rect = { x: 100, y: 200, w: 120, h: 80, radius: 0, font_px: 20, outline: [], text: [150, 230, 16, 10], ko: long };
    const shared = { ...rect, y: 400, outline: [[100, 400], [220, 400], [220, 480], [100, 480]], text: [150, 430, 16, 10],
      parts: [{ text: [110, 430, 16, 10], font_px: 8, ko: long }, { text: [190, 430, 16, 10], font_px: 8, ko: long }] };
    const invoke = vi.fn().mockResolvedValue(null).mockResolvedValueOnce({ ...page(null), boxes: [rect, shared] });
    window.__TAURI_INTERNALS__ = { invoke };
    // The lines overflow at every size: four times the area's height.
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(10);
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(1000);
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 15, 16, 10));
    vi.spyOn(Range.prototype, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 16, 40));
    pagePicture('https://cdn.example/5.jpg');

    new Function(script)();

    const texts = await vi.waitFor(() => {
      const t = [...document.querySelectorAll('.rst-text')];
      expect(t).toHaveLength(3);
      expect(t.every((x) => x.style.transform)).toBe(true);
      return t;
    });
    // Each in its own area, as placed, scaled to a quarter: the lines' height to the area's.
    expect(texts.map((t) => [t.style.left, t.style.top, t.style.width, t.style.height, t.style.overflow, t.style.transform])).toEqual([
      ['41.6667%', '37.5%', '13.3333%', '12.5%', 'visible', 'scale(0.25)'],
      ['8.33333%', '37.5%', '13.3333%', '12.5%', 'visible', 'scale(0.25)'],
      ['75%', '37.5%', '13.3333%', '12.5%', 'visible', 'scale(0.25)'],
    ]);
    expect(texts.map((t) => Number(parseFloat(t.style.fontSize).toFixed(3)))).toEqual([7, 2.8, 2.8]);
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

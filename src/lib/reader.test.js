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

let observe;
let frames;

beforeEach(() => {
  observe = vi.spyOn(MutationObserver.prototype, 'observe');
  frames = [];
  vi.stubGlobal('setInterval', vi.fn());
  vi.stubGlobal('requestAnimationFrame', (f) => {
    const timer = setTimeout(f, 0);
    frames.push(timer);
    return timer;
  });
  // jsdom lays nothing out: the lines' extent is empty, as no spill.
  Range.prototype.getBoundingClientRect ??= () => new DOMRect();
  Range.prototype.getClientRects ??= () => [];
});

afterEach(() => {
  for (const observer of observe.mock.instances) observer.disconnect();
  for (const timer of frames) clearTimeout(timer);
  document.head.replaceChildren();
  document.body.replaceChildren();
  history.replaceState(null, '', '/');
  delete window.__rstReader;
  delete window.__TAURI_INTERNALS__;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('reader page script', () => {
  it.each([
    ['gemini_quota', 'Gemini 요청 한도에 도달했습니다.', 1],
    ['gemini_busy', 'Gemini 서버가 혼잡하거나 일시적으로 응답할 수 없습니다.', 2],
    ['gemini_refused', 'Gemini가 번역 요청을 거절했습니다.', 1],
    ['gemini_refused_400', 'Gemini가 번역 요청을 거절했습니다 (HTTP 400).', 1],
    ['gemini_refused_404', 'Gemini가 요청한 모델 또는 주소를 찾지 못했습니다 (HTTP 404).', 1],
    ['gemini_dev_unreachable', '개발용 Gemini 주소에 연결하지 못했습니다.', 1],
    ['gemini_unreachable', 'Gemini에 닿지 못했습니다. 인터넷 연결을 확인해 주세요.', 2],
    ['gemini_unreadable', 'Gemini 답을 읽지 못했습니다. 그림을 눌러 다시 번역합니다.', 2],
    ['key_invalid', 'Gemini API 키가 맞지 않습니다. 설정에서 확인해 주세요.', 1],
    ['toString', '앱에서 번역을 요청하지 못했습니다.', 1],
    [new Error('IPC rejected the call'), '앱에서 번역을 요청하지 못했습니다.', 1],
  ])('shows the cause for %s and applies its retry policy', async (rejection, message, calls) => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('cors')));
    const invoke = vi.fn().mockRejectedValue(rejection);
    window.__TAURI_INTERNALS__ = { invoke };
    pagePicture('https://cdn.example/failure.jpg');

    new Function(script)();

    const chip = await vi.waitFor(() => {
      const chip = document.querySelector('.rst-chip');
      expect(chip?.textContent).toBe('번역 실패 · 눌러서 다시');
      return chip;
    });
    const status = document.querySelector('.rst-bar [role=status]');
    expect(status.hidden).toBe(false);
    expect(status.textContent).toBe(message);
    expect(invoke).toHaveBeenCalledTimes(calls);
    expect(invoke.mock.calls[0][0]).toBe('post_reader_page');
    if (calls === 2) expect(invoke.mock.calls[1]).toEqual(invoke.mock.calls[0]);

    invoke.mockResolvedValueOnce(page(null));
    chip.click();
    await vi.waitFor(() => expect(document.querySelector('.rst-chip')).toBeNull());
    expect(invoke).toHaveBeenCalledTimes(calls + 1);
    expect(status.hidden).toBe(true);
  });

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

  // A site's own sheet may cap pictures and canvases at their box's width: a
  // tone wider than its box would be drawn squeezed, off the letters it hides.
  it.each([
    ['no rule of its own', ''],
    ['canvases capped at their box', 'canvas{max-width:100%;max-height:100%}'],
    ['canvases capped by a more specific rule', 'div div canvas{max-width:100%;max-height:100%}'],
    ['canvases capped by an important rule', 'body canvas{max-width:90%!important;max-height:90%!important}'],
  ])('draws a tone cover at the size set over a site with %s', async (name, rules) => {
    const sheet = document.createElement('style');
    sheet.textContent = rules;
    document.head.appendChild(sheet);
    vi.stubGlobal('createImageBitmap', vi.fn(async () => ({ width: 10, height: 10 })));
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({ drawImage: vi.fn() });
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('cors'); }));
    const art = { x: 100, y: 400, w: 200, h: 100, radius: 0, font_px: 20, ko: '야호', outline: [], text: [100, 400, 200, 100], patch: { at: [80, 390, 240, 120], png: 'AAAA' } };
    window.__TAURI_INTERNALS__ = { invoke: vi.fn().mockResolvedValue(null).mockResolvedValueOnce({ ...page(null), boxes: [art] }) };
    pagePicture(`https://cdn.example/site-${name.replaceAll(' ', '-')}.jpg`);

    new Function(script)();

    const patch = await vi.waitFor(() => {
      const p = document.querySelector('.rst-patch');
      expect(p).not.toBeNull();
      return p;
    });
    expect([patch.style.width, patch.style.height]).toEqual(['120%', '120%']);
    const drawn = getComputedStyle(patch);
    expect([drawn.maxWidth, drawn.maxHeight]).toEqual(['none', 'none']);
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
  // its own lettering is smaller (a spread across a phone), Korean as large
  // as that lettering there (`lettering_px`). The picture is shown at half
  // its width: 20 page px of lettering is 10 on screen, 8 is 4.
  // Korean too long for its text area even at the page's least size. Three
  // pages, one each: lettering of 20 px (least 7), of 8 px (least 4), and
  // none found (least 7); on each a white rectangle cover, a white balloon
  // with its outline holding a line in two shares, and letters toned on the
  // art. The rule, whatever the web view and wherever the lines overflow:
  // - in a white cover the lines, kept whole and shown, are drawn inside
  //   their area, scaled below the least size if need be;
  // - on the art the size drawn (set size times any scale) is the least
  //   size, never below it: a web view drawing no text under its own
  //   minimum (Android's 8 px) is scaled back to it, no further.
  // (web view, its minimum px, the lines' extent as laid out, the art's transforms)
  const forms = [
    ['drawing the size set, lines overflowing round the area', 0, [0, 0, 16, 40], ['', '', '']],
    ['drawing the size set, lines overflowing below the area', 0, [0, 15, 16, 40], ['', '', '']],
    ['with an 8 px minimum, lines overflowing round the area', 8, [0, 0, 16, 40], ['scale(0.875)', 'scale(0.5)', 'scale(0.875)']],
    ['with an 8 px minimum, lines too wide and overflowing below', 8, [-8, 15, 32, 40], ['scale(0.875)', 'scale(0.5)', 'scale(0.875)']],
  ];
  it.each(forms)('keeps Korean too long for its text area inside a white cover, and at the least size on the art, in a web view %s', async (_, minimum, extent, artTransforms) => {
    const real = window.getComputedStyle.bind(window);
    vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) => {
      const s = real(el, pseudo);
      if (!el.classList?.contains('rst-text')) return s;
      return new Proxy(s, { get: (o, k) => (k === 'fontSize' ? `${Math.max(parseFloat(el.style.fontSize) || 0, minimum)}px` : typeof o[k] === 'function' ? o[k].bind(o) : o[k]) });
    });
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('cors'); }));
    // A rectangle cover, and a balloon with its outline holding a line in
    // two shares: each a 16 x 10 text area.
    const long = '가'.repeat(100);
    const rect = { x: 100, y: 200, w: 120, h: 80, radius: 0, font_px: 20, outline: [], text: [150, 230, 16, 10], ko: long };
    const shared = { ...rect, y: 400, outline: [[100, 400], [220, 400], [220, 480], [100, 480]], text: [150, 430, 16, 10],
      parts: [{ text: [110, 430, 16, 10], font_px: 20, ko: long }, { text: [190, 430, 16, 10], font_px: 20, ko: long }] };
    const art = { ...rect, y: 600, text: [150, 630, 16, 10], patch: { at: [140, 620, 40, 30], png: 'AAAA' } };
    vi.stubGlobal('createImageBitmap', vi.fn(async () => ({ width: 1, height: 1 })));
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({ drawImage: vi.fn() });
    const invoke = vi.fn().mockResolvedValue(null);
    for (const lettering_px of [20, 8, null]) invoke.mockResolvedValueOnce({ ...page(null), lettering_px, boxes: [rect, shared, art] });
    window.__TAURI_INTERNALS__ = { invoke };
    // The lines overflow at every size: four times the area's height.
    const area = new DOMRect(0, 15, 16, 10);
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(10);
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockReturnValue(1000);
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(area);
    vi.spyOn(Range.prototype, 'getBoundingClientRect').mockReturnValue(new DOMRect(...extent));
    // Pictures of their own for each form: a page script left from another
    // test keeps the layouts of the pictures it has seen.
    for (const n of [5, 6, 7]) pagePicture(`https://cdn.example/least-${minimum}-${extent.join('_')}-${n}.jpg`);

    new Function(script)();

    const texts = await vi.waitFor(() => {
      const t = [...document.querySelectorAll('.rst-text')];
      expect(t).toHaveLength(12);
      expect(t.every((x) => x.style.overflow === 'visible')).toBe(true);
      return t;
    });
    // Each in its own area, as placed, set at the page's least size.
    expect(texts.map((t) => [t.style.left, t.style.top, t.style.width, t.style.height])).toEqual([0, 1, 2].flatMap(() => [
      ['41.6667%', '37.5%', '13.3333%', '12.5%'],
      ['8.33333%', '37.5%', '13.3333%', '12.5%'],
      ['75%', '37.5%', '13.3333%', '12.5%'],
      ['41.6667%', '37.5%', '13.3333%', '12.5%'],
    ]));
    expect(texts.map((t) => Number(parseFloat(t.style.fontSize).toFixed(3)))).toEqual([7, 7, 7, 7, 4, 4, 4, 4, 7, 7, 7, 7]);
    const white = texts.filter((t) => t.parentElement.querySelector('.rst-fill'));
    const toned = texts.filter((t) => !t.parentElement.querySelector('.rst-fill'));
    expect([white.length, toned.length]).toEqual([9, 3]);
    // In a white cover: the lines, scaled round the area's centre and moved,
    // lie inside the area.
    const [l, eps] = [new DOMRect(...extent), 1e-6];
    for (const t of white) {
      const m = t.style.transform.match(/^translate\((-?[\d.e-]+)%(?:, (-?[\d.e-]+)%)?\) scale\(([\d.]+)\)$/);
      expect(m, t.style.transform).not.toBeNull();
      const [tx, ty, k] = [Number(m[1]), Number(m[2] ?? 0), Number(m[3])];
      const cx = area.x + area.width / 2 + k * (l.x + l.width / 2 - (area.x + area.width / 2)) + (tx / 100) * area.width;
      const cy = area.y + area.height / 2 + k * (l.y + l.height / 2 - (area.y + area.height / 2)) + (ty / 100) * area.height;
      expect(cx - (k * l.width) / 2).toBeGreaterThanOrEqual(area.left - eps);
      expect(cx + (k * l.width) / 2).toBeLessThanOrEqual(area.right + eps);
      expect(cy - (k * l.height) / 2).toBeGreaterThanOrEqual(area.top - eps);
      expect(cy + (k * l.height) / 2).toBeLessThanOrEqual(area.bottom + eps);
    }
    // On the art: drawn at the least size, scaled only where the web view
    // draws it larger, never below it.
    expect(toned.map((t) => t.style.transform)).toEqual(artTransforms);
    const drawn = (t) => parseFloat(getComputedStyle(t).fontSize) * (t.style.transform ? Number(t.style.transform.slice(6, -1)) : 1);
    expect(toned.map((t) => Number(drawn(t).toFixed(3)))).toEqual([7, 4, 7]);
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

// The reader's page script (src-tauri/src/reader.js) on a page with one manga
// picture, the app's commands stood in for by `invoke`.
import { describe, expect, it, vi } from 'vitest';
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
  boxes: [{ x: 60, y: 80, w: 120, h: 60, radius: 6, font_px: 20, ko: '안녕' }],
  save_error,
});

describe('reader page script', () => {
  it('shows a page it could not keep in the library, says so, and saves it on a tap', async () => {
    vi.stubGlobal('requestAnimationFrame', (f) => setTimeout(f, 0));
    // The site does not hand over the bytes: the app fetches them itself.
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('cors'); }));
    const invoke = vi.fn().mockResolvedValueOnce(page('storage_full')).mockResolvedValueOnce(page(null));
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
  });
});

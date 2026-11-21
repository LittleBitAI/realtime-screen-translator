// The reader's page script (src-tauri/src/reader.js) with an imported
// chapter picture that fails to decode. One chapter per file: a script
// started keeps watching the document.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { blank, openChapter } from './reader-chapter-fixture.js';

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

describe('reader page script, a chapter picture that fails to decode', () => {
  // Bytes that are no picture: page 11 is marked unreadable, with a way to
  // read it again, and no longer holds back page 12.
  it('translates the later page when an earlier picture fails to decode', async () => {
    const { sent } = openChapter({ ids: [11, 12], broken: [11], page: async () => blank });
    await vi.waitFor(() => expect(sent()).toEqual([12]), { timeout: 2000 });
    const chip = document.querySelector('img[data-picture-id="11"]').parentElement.querySelector('.rst-chip');
    expect(chip?.textContent).toContain('그림을 읽을 수 없습니다');
  });
});

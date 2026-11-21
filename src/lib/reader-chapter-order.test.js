// The reader's page script (src-tauri/src/reader.js) sending an imported
// chapter's pages to the app. Apart from the other script tests: a reader
// started there keeps watching the document and would send these pages too.
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

describe('reader page script, chapter pages in order', () => {
  // An imported chapter: the app can only have page 2 wait for page 1 once
  // page 1 is sent, so page 2 is not sent while page 1's picture loads.
  it("sends chapter pages in reading order even when a later page's picture loads sooner", async () => {
    const { sent } = openChapter({
      ids: [11, 12],
      image: async (id) => {
        if (id === 11) await new Promise((ok) => setTimeout(ok, 50));
        return new Uint8Array([1, 2, 3]).buffer;
      },
      page: async () => blank,
    });
    await vi.waitFor(() => expect(sent()).toHaveLength(2), { timeout: 2000 });
    expect(sent()).toEqual([11, 12]);
  });
});

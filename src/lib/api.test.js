import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError } from './api.js';

afterEach(() => vi.unstubAllGlobals());

const answer = (status, body) => vi.fn(async () => new Response(JSON.stringify(body), { status }));

describe('api over HTTP', () => {
  it('returns the body of a success', async () => {
    vi.stubGlobal('fetch', answer(200, { on: false }));
    expect(await api.status()).toEqual({ on: false });
  });

  it('turns an error answer into its code', async () => {
    vi.stubGlobal('fetch', answer(400, { error: 'key_format' }));
    await expect(api.putKey('x')).rejects.toMatchObject({ code: 'key_format', status: 400 });
  });

  it('calls a 500 without a body internal', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('oops', { status: 500 })));
    await expect(api.status()).rejects.toMatchObject({ code: 'internal', status: 500 });
  });

  it('calls a failed fetch unreachable', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('down'); }));
    const e = await api.putSourceLang('en').catch((e) => e);
    expect(e).toBeInstanceOf(ApiError);
    expect(e.code).toBe('unreachable');
  });

  it('sends the paths and bodies of the design', async () => {
    const f = answer(200, {});
    vi.stubGlobal('fetch', f);
    await api.books();
    await api.books(7);
    await api.exportBook(3, 'webp');
    await api.putKey('k');
    await api.putSourceLang('en');
    expect(f.mock.calls.map(([p, o]) => [o.method, p, o.body])).toEqual([
      ['GET', '/books?limit=20', undefined],
      ['GET', '/books?limit=20&before=7', undefined],
      ['POST', '/books/3/export', '{"format":"webp"}'],
      ['PUT', '/settings/gemini-key', '{"key":"k"}'],
      ['PUT', '/settings/source-lang', '{"lang":"en"}'],
    ]);
    // A browser cannot show another site with the translation on it.
    expect(api.canOpenSites()).toBe(false);
    expect(await api.previewSrc('strips/1/0-5.webp')).toBe('/files/strips/1/0-5.webp');
    // The backend gives the strip's full path in the data folder.
    expect(await api.previewSrc('\\\\?\\C:\\data\\strips\\2\\0-9.webp')).toBe('/files/strips/2/0-9.webp');
    expect(await api.previewSrc('/data/user/0/app/strips/3/0-9.webp')).toBe('/files/strips/3/0-9.webp');
  });

  it('offers each exported picture for download from the host', () => {
    // The backend names the files it wrote in the data folder's `export/`.
    const out = { count: 2, files: ['C:\\data\\export\\translated-1.png', '/data/export/translated-2.webp'] };
    expect(api.exportLinks(out)).toEqual([
      { href: '/files/export/translated-1.png', name: 'translated-1.png' },
      { href: '/files/export/translated-2.webp', name: 'translated-2.webp' },
    ]);
  });
});

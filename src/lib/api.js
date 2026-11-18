// The backend of docs/design/api.yaml. Inside the Tauri app (phone and PC)
// every call is a Tauri command under its A1 name; in a browser it is the
// HTTP API the web host serves on the same port as these screens.
import { invoke } from '@tauri-apps/api/core';

/** An answer that is not a success: `code` is the design's error code. */
export class ApiError extends Error {
  constructor(code, status) {
    super(code);
    this.code = code;
    this.status = status;
  }
}

const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export async function http(method, path, body) {
  let res;
  try {
    res = await fetch(path, {
      method,
      headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    throw new ApiError('unreachable', 0);
  }
  let data = null;
  try {
    data = await res.json();
  } catch {
    data = null;
  }
  if (!res.ok) throw new ApiError(data?.error ?? 'internal', res.status);
  return data;
}

async function call(cmd, args, method, path, body) {
  if (!inTauri()) {
    if (!method) throw new ApiError('native_only', 0);
    return http(method, path, body);
  }
  try {
    return await invoke(cmd, args);
  } catch (e) {
    throw new ApiError(typeof e === 'string' ? e : 'internal', 500);
  }
}

export const api = {
  status: () => call('get_status', {}, 'GET', '/status'),
  putKey: (key) => call('put_settings_gemini_key', { key }, 'PUT', '/settings/gemini-key', { key }),
  putSourceLang: (lang) => call('put_settings_source_lang', { lang }, 'PUT', '/settings/source-lang', { lang }),
  /** Only the installed app can show another site with the translation on it (P12). */
  canOpenSites: inTauri,
  openSite: (url) => call('post_reader_open', { url }, 'POST', '/reader/open', { url }),
  importChapter: () => call('post_chapter_import', {}),
  books: (before = null) =>
    call('get_books', { limit: 20, before }, 'GET', before == null ? '/books?limit=20' : `/books?limit=20&before=${before}`),
  exportBook: (bookId, format) =>
    call('post_books_export', { book_id: bookId, format }, 'POST', `/books/${bookId}/export`, { format }),
  /** Where a book's preview (`preview_path`, a file in the app's data folder) can be shown from. */
  previewSrc: (path) => {
    const rel = dataPart(path, 'strips');
    return inTauri() ? call('get_preview', { path: rel }) : Promise.resolve(`/files/${rel}`);
  },
  /**
   * In a browser, where the reader downloads each exported picture from
   * (the web host offers them under `/files/export/`). Inside the app the
   * pictures are already in the gallery or Downloads, so there are none.
   */
  exportLinks: (out) =>
    inTauri() ? [] : out.files.map((f) => ({ href: `/files/${dataPart(f, 'export')}`, name: dataPart(f, 'export').slice('export/'.length) })),
};

/** The part of a path in the app's data folder from its `folder/` on, which the hosts serve. */
export function dataPart(path, folder) {
  const p = path.replace(/\\/g, '/');
  const at = p.lastIndexOf(`/${folder}/`);
  return at >= 0 ? p.slice(at + 1) : p;
}

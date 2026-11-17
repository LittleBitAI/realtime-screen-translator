import { describe, expect, it } from 'vitest';
import { bookName, exportDone, exportError, keyError, langText, openError } from './text.js';

describe('text', () => {
  it('names the source language', () => {
    expect(langText('en')).toBe('영어');
    expect(langText('ja')).toBe('일본어');
  });

  it('names a book by its site page, or by when capture began', () => {
    expect(bookName({ title: 'FX Fighter Kurumi-chan - Chapter 54', started_at: 0 })).toBe('FX Fighter Kurumi-chan - Chapter 54');
    expect(bookName({ title: '2026-11-17 08:00', started_at: Date.UTC(2026, 10, 17, 8) })).toMatch(/에 켠 번역본$/);
  });

  it('says where an export went', () => {
    expect(exportDone({ count: 2, files: ['C:\\out\\a.png', 'C:\\out\\b.png'] })).toBe('그림 2장을 C:\\out에 저장했습니다.');
    expect(exportDone({ count: 1, files: ['Pictures/rst/a.webp'] })).toBe('그림 1장을 Pictures/rst에 저장했습니다.');
  });

  it('explains each error code', () => {
    expect(keyError('key_format')).toContain('키 모양');
    expect(exportError('empty_book')).toContain('내보낼 그림이 없습니다');
    expect(openError('key_missing')).toContain('키가 없습니다');
    expect(openError('bad_url')).toContain('주소가 맞지 않습니다');
    expect(openError('whatever')).toContain('연결하지 못했습니다');
  });
});

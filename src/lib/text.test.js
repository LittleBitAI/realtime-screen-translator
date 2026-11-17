import { describe, expect, it } from 'vitest';
import { exportDone, exportError, keyError, modeText, permissionText, startError } from './text.js';

describe('text', () => {
  it('names the tracking mode', () => {
    expect(modeText({ on: false, mode: 'scroll' })).toBe('없음');
    expect(modeText({ on: true, mode: 'scroll' })).toBe('스크롤');
    expect(modeText({ on: true, mode: 'slide' })).toBe('슬라이딩');
    expect(modeText({ on: true, mode: null })).toBe('화면을 살피는 중');
  });

  it('says which permissions are in place', () => {
    expect(permissionText({ capture_allowed: true, overlay_allowed: true })).toBe('모두 허용됨');
    expect(permissionText({ capture_allowed: false, overlay_allowed: false })).toBe('없음');
  });

  it('says where an export went', () => {
    expect(exportDone({ count: 2, files: ['C:\\out\\a.png', 'C:\\out\\b.png'] })).toBe('그림 2장을 C:\\out에 저장했습니다.');
    expect(exportDone({ count: 1, files: ['Pictures/rst/a.webp'] })).toBe('그림 1장을 Pictures/rst에 저장했습니다.');
  });

  it('explains each error code', () => {
    expect(keyError('key_format')).toContain('키 모양');
    expect(exportError('empty_book')).toContain('내보낼 그림이 없습니다');
    expect(startError('key_missing')).toContain('키가 없습니다');
    expect(startError('whatever')).toContain('연결하지 못했습니다');
  });
});

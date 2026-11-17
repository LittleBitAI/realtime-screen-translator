// What the screens say, from the backend's values and error codes.

/** The status notice (P1 `notice`): what went wrong and what to do. */
export const notices = {
  key_invalid: { why: 'Gemini API 키가 맞지 않습니다.', todo: '설정에서 키를 다시 넣어 주세요.', options: true },
  capture_blocked: { why: '보던 앱이 화면 캡처를 막아 두었습니다.', todo: '그 앱은 번역할 수 없습니다. 다른 앱에서 읽어 주세요.' },
  gemini_unreachable: { why: 'Gemini에 닿지 않습니다.', todo: '인터넷 연결을 확인해 주세요. 다시 닿으면 이어서 번역합니다.' },
  detector_missing: { why: '말풍선을 찾는 모델을 아직 받지 못했습니다.', todo: '인터넷에 연결한 채로 잠시 기다려 주세요.' },
};

/** Why turning translation on did not work (P4 errors). */
export function startError(code) {
  switch (code) {
    case 'key_missing':
      return 'Gemini API 키가 없습니다. 설정에서 키를 먼저 넣어 주세요.';
    case 'permission_required':
      return '화면 캡처와 다른 앱 위에 표시 권한이 있어야 번역을 켤 수 있습니다.';
    case 'capture_unsupported':
      return captureUnsupported;
    case 'key_invalid':
      return notices.key_invalid.why + ' ' + notices.key_invalid.todo;
    default:
      return reachError();
  }
}

export const reachError = () => '앱과 연결하지 못했습니다. 잠시 뒤 다시 해 주세요.';

/** Where capture is impossible (a browser): P1 `capture_supported` false, P4 `capture_unsupported`. */
export const captureUnsupported =
  '이 브라우저에서는 다른 앱의 화면을 읽거나 그 위에 번역을 띄울 수 없어 번역을 켤 수 없습니다. 휴대폰이나 PC에 설치한 앱에서 켜 주세요.';

/** Why an export did not work (P11 errors). */
export function exportError(code) {
  switch (code) {
    case 'storage_full':
      return '저장 공간이 모자랍니다. 공간을 비운 뒤 다시 내보내 주세요.';
    case 'storage_denied':
      return '갤러리에 저장할 권한이 없습니다. 권한을 허용한 뒤 다시 내보내 주세요.';
    case 'empty_book':
      return '이 번역본에는 내보낼 그림이 없습니다.';
    default:
      return '내보내지 못했습니다. 다시 내보내 주세요.';
  }
}

/** Why a key was not saved (P2 errors). */
export function keyError(code) {
  switch (code) {
    case 'key_format':
      return '키 모양이 맞지 않습니다. 발급받은 키를 다시 확인해 주세요.';
    case 'secure_storage_unavailable':
      return '이 기기에는 키를 안전하게 둘 곳이 없어 저장하지 않았습니다.';
    default:
      return reachError();
  }
}

/** The tracking mode on the first screen (P1 `mode`). */
export function modeText(status) {
  if (!status.on) return '없음';
  if (status.mode === 'scroll') return '스크롤';
  if (status.mode === 'slide') return '슬라이딩';
  return '화면을 살피는 중';
}

export function permissionText(status) {
  if (status.capture_allowed && status.overlay_allowed) return '모두 허용됨';
  if (status.capture_allowed) return '화면 캡처만 허용됨';
  if (status.overlay_allowed) return '다른 앱 위에 표시만 허용됨';
  return '없음';
}

export const allowed = (yes) => (yes ? '허용됨' : '없음');

export const kindText = (kind) => (kind === 'stitched' ? '이어 붙인 한 장' : '페이지별 그림');

const when = new Intl.DateTimeFormat('ko-KR', { dateStyle: 'medium', timeStyle: 'short' });

/** A book's name, from when translation was turned on for it. */
export const bookName = (startedAt) => `${when.format(new Date(startedAt))}에 켠 번역본`;

/** An export in a browser: the pictures are made and wait to be downloaded. */
export const exportReady = (count) => `그림 ${count}장을 만들었습니다. 아래를 눌러 이 기기에 내려받으세요.`;

/** Where an export went: the folder (or gallery place) of its first file. */
export function exportDone(out) {
  const first = out.files[0] ?? '';
  const cut = Math.max(first.lastIndexOf('/'), first.lastIndexOf('\\'));
  const place = cut > 0 ? first.slice(0, cut) : first;
  return `그림 ${out.count}장을 ${place}에 저장했습니다.`;
}

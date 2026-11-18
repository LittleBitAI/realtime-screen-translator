// What the screens say, from the backend's values and error codes.

/** The status notice (P1 `notice`): what went wrong and what to do. */
export const notices = {
  key_invalid: { why: 'Gemini API 키가 맞지 않습니다.', todo: '설정에서 키를 다시 넣어 주세요.', options: true },
  gemini_unreachable: { why: 'Gemini에 닿지 않습니다.', todo: '인터넷 연결을 확인해 주세요. 페이지를 다시 넘기면 이어서 번역합니다.' },
};

/** Why a site did not open in the reader. */
export function openError(code) {
  switch (code) {
    case 'bad_url':
      return '주소가 맞지 않습니다. 만화 페이지의 주소를 그대로 붙여 넣어 주세요.';
    case 'key_missing':
      return 'Gemini API 키가 없습니다. 설정에서 키를 먼저 넣어 주세요.';
    default:
      return reachError();
  }
}

export function importError(code) {
  switch (code) {
    case 'empty_folder':
      return '선택한 폴더에 JPG, PNG 또는 WebP 그림이 없습니다.';
    case 'bad_image':
    case 'image_unreachable':
      return '폴더의 그림을 읽을 수 없습니다. 파일을 확인해 주세요.';
    case 'image_too_large':
      return '20MB가 넘는 그림이 있습니다. 파일 크기를 확인해 주세요.';
    case 'image_dimensions_too_large':
      return '가로와 세로를 곱해 8,388,608픽셀이 넘는 그림이 있습니다. 그림의 해상도를 낮춰 주세요.';
    case 'storage_full':
      return '저장 공간이 모자랍니다. 공간을 비운 뒤 다시 가져와 주세요.';
    case 'storage_denied':
      return '앱이 저장 폴더에 쓰지 못했습니다.';
    default:
      return '폴더를 가져오지 못했습니다. 잠시 뒤 다시 해 주세요.';
  }
}

export const reachError = () => '앱과 연결하지 못했습니다. 잠시 뒤 다시 해 주세요.';

/** In a browser no other site can be opened inside these screens. */
export const readerUnsupported =
  '브라우저에서는 다른 사이트를 이 화면 안에 열 수 없습니다. 휴대폰이나 PC에 설치한 앱에서 열어 주세요. 번역본과 설정은 여기서도 볼 수 있습니다.';

/** The language the reader translates from (P1 `source_lang`). */
export const langText = (lang) => (lang === 'en' ? '영어' : '일본어');

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

export const kindText = (kind) => (kind === 'stitched' ? '이어 붙인 한 장' : '페이지별 그림');

const when = new Intl.DateTimeFormat('ko-KR', { dateStyle: 'medium', timeStyle: 'short' });

/**
 * A book's name: the site page's title for one the reader saved, or, for
 * one screen capture saved (titled by its start time), when it began.
 */
export const bookName = (book) =>
  book.title && (book.url || book.imported || !/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/.test(book.title))
    ? book.title : `${when.format(new Date(book.started_at))}에 켠 번역본`;

/** An export in a browser: the pictures are made and wait to be downloaded. */
export const exportReady = (count) => `그림 ${count}장을 만들었습니다. 아래를 눌러 이 기기에 내려받으세요.`;

/** Where an export went: the folder (or gallery place) of its first file. */
export function exportDone(out) {
  const first = out.files[0] ?? '';
  const cut = Math.max(first.lastIndexOf('/'), first.lastIndexOf('\\'));
  const place = cut > 0 ? first.slice(0, cut) : first;
  return `그림 ${out.count}장을 ${place}에 저장했습니다.`;
}

# realtime-screen-translator

휴대폰과 PC 앱 안에서 연 만화 사이트의 일본어·영어 만화를 실시간으로 읽어, 말풍선 위에 한국어 번역을 얹어 주는 프로그램입니다. 가장 많이 쓰는 휴대폰(갤럭시 S21 기준)에 먼저 맞췄고, 번역을 페이지 그림에 붙여 위아래 스크롤과 앞뒤 슬라이딩 모두 그림을 따라가며, 이미 번역한 페이지와 효과음은 다시 번역하지 않습니다. 화면 캡처로 다른 앱 위에 번역을 띄우던 길은 백엔드에 남아 있지만, 앱 화면에서는 더 이상 켜지 않습니다. 번역은 사용자가 넣은 Gemini API 키로 Gemini 3.1 Flash Lite가 맡고, 읽은 화면은 기기 안의 번역본에 쌓여 그림 파일로 내보낼 수 있습니다.

화면은 세 개입니다 (`docs/design/frontend.md`).

- `/` — 만화 페이지 주소를 넣어 앱 안 리더로 엽니다. 키와 원문 언어, Gemini 요청 횟수, 문제가 생겼을 때의 안내를 한눈에 봅니다.
- `/library` — 저장된 번역본을 새것부터 훑어보고 PNG나 WebP 그림으로 내보냅니다.
- `/options` — 자기 Gemini API 키를 넣거나 바꾸거나 지우고, 원문 언어(일본어 / 영어)를 고릅니다.

### 앱 안 리더로 읽기

1. 설정에서 Gemini API 키를 넣고, 원문 언어를 고릅니다. 영어판 만화라면 영어를 고릅니다.
2. 첫 화면에 만화 페이지 주소(예: mangafire의 회차 주소)를 넣고 열기를 누릅니다. 휴대폰에서는 앱 화면이, PC에서는 새 창이 그 사이트가 됩니다.
3. 페이지 그림이 뜨면 그림마다 한 번씩 Gemini에 보내고, 말풍선 위에 흰 shape과 한국어를 그림에 붙여 그립니다. 그래서 위아래로 스크롤하든 옆으로 넘기든 번역이 그림과 함께 움직입니다. 효과음과 간판 글자는 번역하지 않습니다. 화면 앞뒤 1.5화면 안의 그림은 미리 번역합니다.
4. 한 번 번역한 그림은 같은 원문 언어로 다시 보내지 않고, 사이트 페이지마다 번역본 하나로 쌓여 `/library`에서 내보낼 수 있습니다.
5. Use `번역 켜짐` or `번역 꺼짐` on the reader toolbar to show or hide translations. Tap a translation to see the original dialogue briefly, or use `앱으로 돌아가기` to return to the app.

리더가 열린 사이트에는 번역 명령 두 개(`post_reader_page`, `post_reader_close`)만 열려 있고, 키와 설정, 내보내기 명령은 앱 자신의 화면에서만 부를 수 있습니다. 광고가 여는 새 창과 다른 앱으로 가는 주소는 막습니다.

백엔드 설계는 `docs/design/backend.md`, API는 `docs/design/api.yaml`에 있습니다.

## 준비

필요한 것은 Git, Rust(stable)와 C/C++ 빌드 도구, Node.js와 Corepack, cargo-nextest입니다. 화면 쪽 패키지는 Corepack이 `package.json`에 적힌 pnpm 12.21.0으로 설치하므로 pnpm을 따로 깔지 않습니다.

### Windows (PowerShell 7)

명령은 PowerShell 7(`pwsh`)에서 실행합니다. 기본으로 깔린 Windows PowerShell 5.1은 테스트 명령의 `&&`를 알아듣지 못합니다.

```powershell
winget install --id Microsoft.PowerShell
winget install --id Git.Git
winget install --id Rustlang.Rustup
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id OpenJS.NodeJS.LTS
```

설치가 끝나면 새 `pwsh` 창을 열고:

```powershell
npm install --global corepack@latest
cargo install cargo-nextest --locked
git clone https://github.com/LittleBitAI/realtime-screen-translator.git
cd realtime-screen-translator
corepack pnpm install --frozen-lockfile
mkdir artifacts
```

### macOS (터미널)

[Homebrew](https://brew.sh)가 깔려 있다고 봅니다.

```sh
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
brew install git node
npm install --global corepack@latest
cargo install cargo-nextest --locked
git clone https://github.com/LittleBitAI/realtime-screen-translator.git
cd realtime-screen-translator
corepack pnpm install --frozen-lockfile
mkdir -p artifacts
```

`artifacts/`는 테스트 보고서가 놓이는 폴더라 Git이 추적하지 않습니다. 새로 받은 저장소에서는 위처럼 한 번 만들어 두어야 테스트가 보고서를 쓸 수 있습니다. Corepack이 처음 pnpm을 내려받을 때 계속할지 물으면 `Y`로 답합니다.

## 실행

```sh
node scripts/live.mjs
```

패키지가 없으면 설치하고, 화면을 `dist/`로 빌드한 뒤 Rust 웹 호스트(`src-tauri/src/bin/web-host.rs`)를 빌드해 띄웁니다. 처음에는 Rust 빌드에 몇 분 걸립니다. `listening on http://localhost:8080`이 나오면 브라우저에서 <http://localhost:8080>을 엽니다. 화면과 API가 같은 포트에 있고, `/library`와 `/options`도 주소로 바로 열립니다. 끌 때는 `Ctrl+C`를 누릅니다.

포트와 데이터 폴더는 환경 변수 `PORT`와 `APP_DATA_DIR`로 바꿉니다. 기본값은 포트 8080, 데이터는 시스템 임시 폴더 안의 `rst-web`입니다.

```powershell
# Windows (pwsh)
$env:PORT = "3000"; $env:APP_DATA_DIR = "$env:LOCALAPPDATA\rst-web"; node scripts/live.mjs
```

```sh
# macOS
PORT=3000 APP_DATA_DIR="$HOME/rst-web" node scripts/live.mjs
```

브라우저는 다른 사이트의 페이지에 번역을 붙일 수 없으므로, 브라우저판 첫 화면은 사이트 열기 대신 설치한 앱(Android, Windows)에서 열라는 안내를 보여 줍니다. 키와 원문 언어 설정, 번역본 목록과 내보내기는 브라우저판에서도 됩니다. `e2e/seed.json`만 `POST /test/simulated-device`로 대역 캡처 기기를 붙여 휴대폰처럼 프레임과 화면 조각을 보냅니다. 내보낸 그림은 `APP_DATA_DIR/export`에 놓이고, 번역본 화면에 그림마다 뜨는 내려받기 버튼으로 브라우저가 있는 기기에 받습니다. 실제로 보던 앱 위에 번역을 띄우는 일은 같은 화면과 백엔드를 쓰는 Tauri 앱(Windows, Android)이 합니다. 그 구성은 아래 [구조](#구조)에 있습니다.

### Android APK (Windows)

JDK 17과 Android SDK(command-line tools, `platform-tools`, `build-tools;34.0.0`, `ndk;27.2.12479018`)를 깔고 `JAVA_HOME`, `ANDROID_HOME`, `NDK_HOME`을 맞춥니다. Android 프로젝트는 `src-tauri/gen/android`에 커밋되어 있습니다.

```sh
cargo tauri android build --apk --target aarch64 --features app -c '{"build":{"beforeBuildCommand":"corepack pnpm build"}}'
```

Windows 개발자 모드가 꺼져 있으면 마지막 심볼릭 링크 단계에서 멈춥니다. 그때는 `target/aarch64-linux-android/release/librealtime_screen_translator.so`를 `src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a/`에 복사하고, `src-tauri/gen/android`에서 `./gradlew.bat assembleUniversalDebug -x rustBuildUniversalDebug -x rustBuildArm64Debug -PabiList=arm64-v8a -ParchList=arm64 -PtargetList=aarch64`를 돌립니다. APK는 `app/build/outputs/apk/universal/debug/app-universal-debug.apk`에 나옵니다.

## 테스트

```sh
cargo run -q -p xtask -- artifacts/junit.xml && corepack pnpm install --frozen-lockfile && corepack pnpm test && corepack pnpm build
```

저장소의 게이트이며 CI(`.github/workflows/ci.yml`)도 push와 pull request마다 Windows에서 같은 명령을 돌립니다. 앞에서부터 차례로:

1. `xtask`가 `cargo nextest run --workspace --exclude xtask --profile ci`로 Rust 테스트를 모두 돌리고, JUnit 보고서를 `artifacts/junit.xml`에 씁니다. 백엔드 계약 테스트(`src-tauri/tests/contract.rs`)는 `contract-host` 바이너리를 따로 띄워 API를 바깥에서 부르고, 나머지 `src-tauri/tests/*.rs`는 트래킹, 이미 한 번역 건너뛰기, 효과음 거르기, 저장과 내보내기 같은 경계 상황을 봅니다. Gemini는 테스트 안의 가짜 서버가 대신합니다.
2. `corepack pnpm install --frozen-lockfile`이 커밋된 `pnpm-lock.yaml` 그대로 패키지를 맞춥니다.
3. `corepack pnpm test`가 Vitest로 API 호출과 오류 코드(`src/lib/api.test.js`), 안내 문구(`src/lib/text.test.js`), 세 화면의 빈·불러오는 중·오류·채워진 상태와 주요 흐름(`src/screens/screens.test.js`)을 확인하고 `artifacts/vitest-junit.xml`을 씁니다.
4. `corepack pnpm build`가 Vite로 화면을 `dist/`에 빌드해 Svelte·Tailwind 오류를 잡습니다.

## 처음 요청

> 나는 다음을 만들고 싶어.
- 실시간 화면 인식 번역 프로그램.
스크롤및 앞뒤로 왔다갔다 할 때를 트래킹을 해야하고, 이미 했던 번역을 또하는것은 피해야해.
또한 효과음 번역도 피해야해.
대표적인 용도는 일본 만화를 한국어로 실시간 번역 하는것이야.
이때, 모바일에서의 최적화가 가장 중요해.(기준 기기는 갤럭시 s21)

번역 모델은 gemini 3.1 flast lite.
이떄 기본적으로 있는 gemini의 안전 가드레일은 off로.(거기에 더해 프리필까지.)

중요한것은 다음과 같아.
- 화면 위 아래 스크롤과 앞뒤로 슬라이딩으로 이전/다음 페이지를 넘기는 방식을 구분할것.
- 화면 위 아래 스크롤 방식에서, 어떻게 트래킹 할것인가?
- 번역하는 화면에서, 어떻게 "이미 했던 번역"임은 어떻게 알것인가?

## 구조

- 백엔드: `src-tauri/`(Rust). `docs/design/api.yaml`의 명령은 `src/backend.rs`에 있고, Tauri 앱 셸은 `app` 기능 뒤의 `src/app.rs`입니다. 계약 테스트는 같은 백엔드를 `contract-host`로, 브라우저판은 `web-host`로 띄웁니다.
- 화면: `src/`(Svelte 5, Vite, Tailwind CSS). Tauri 앱 안에서는 API 호출이 모두 Tauri 명령이 되고, 브라우저에서는 `web-host`로 가는 HTTP가 됩니다(`src/lib/api.js`). `e2e/seed.json`은 빈 데이터 폴더를 번역본 두 개가 있고 번역이 켜진 상태로 채우는 API 호출 목록이며, `python -I e2e/make_seed.py`가 만듭니다.
- 캡처 흐름: `src/pipeline.rs`가 캡처한 프레임으로 P6, P7, P9를 돌립니다. Windows에서는 `src/wgc.rs`가 Windows Graphics Capture로 캡처하고 ONNX Runtime(`ort`)으로 말풍선 모델을 돌립니다. Android에서는 `src-tauri/plugins/screen/android/`의 Kotlin 플러그인이 MediaProjection으로 캡처하고, ONNX Runtime으로 모델을 돌리고, 오버레이와 떠 있는 켜고 끄기 버튼을 그리고, 내보낸 그림을 갤러리(MediaStore, `Pictures/RealtimeTranslator`)에 저장하고, Gemini 키를 Android Keystore로 봉합니다.
- 말풍선 모델: 앱은 Apache-2.0 [comic-text-and-bubble-detector](https://huggingface.co/ogkalu/comic-text-and-bubble-detector)(RT-DETR-v2, int8, 11 MB)를 고정된 주소에서 받습니다. 주소, 리비전, SHA-256은 `src/models.rs`에 있습니다. 처음 켤 때 앱 데이터 폴더의 `models/balloons.onnx`로 뒤에서 받아 체크섬이 맞을 때만 남기고, 그때까지 P1은 `detector_missing`을 알립니다. `RST_DETECTOR_MODEL`로 같은 파일의 다른 사본을 가리킬 수 있습니다. `tests/fixtures/one-balloon.onnx`는 테스트용 대역일 뿐입니다.
- Android 오버레이: 번역된 말풍선마다 터치를 받지 않는 작은 창이라 모든 터치가 보던 앱으로 갑니다(P4). Android 12부터는 다른 앱의 창을 지나는 터치가, 그 창이 80% 이하로 불투명하거나 접근성 오버레이가 아니면 막힙니다. 그래서 Android 12 이상에서 R29의 오버레이 권한은 두 단계입니다. 다른 앱 위에 표시, 그리고 앱의 "번역 오버레이" 접근성 서비스입니다. 이 서비스는 화면 내용을 읽지 않고 흰 shape에 접근성 오버레이 종류만 빌려 주므로, shape이 완전히 불투명해 일본어를 다 가리면서도 터치는 모두 통과합니다. 권한을 물을 때 이 서비스가 켜질 때까지 접근성 설정을 열고, 켜지지 않으면 번역을 시작하지 않습니다(`ScreenPlugin.kt`, `Overlay.kt`, `OverlayService.kt`). 캡처에는 이 오버레이가 담기므로 P6은 프레임을 견줄 때 placement 아래 픽셀을 뺍니다. 켜고 끄기 버튼은 아래쪽 가장자리 근처에 떠 있고 역시 캡처에 담기므로 프레임마다 그 사각형이 같이 옵니다. 저장할 줄이 버튼 뒤에 있으면 버튼을 한 프레임쯤 숨기고, 새 프레임이 같은 화면이면 그 줄을 버튼 없는 새 프레임에서 저장합니다(`Capture.cleanFrame`, `pipeline::same_screen`). 아니면 버튼 주변 줄만 저장하고 나머지는 다시 요청합니다(`pipeline::clear_of`). 휴대폰을 돌리면 미러를 새 화면 크기로 바꾸므로(`VirtualDisplay.resize`와 새 ImageReader) 프레임, placement, 버튼이 화면 좌표를 지키고 P6은 돌린 크기로 새 그림을 시작합니다. 번역이 꺼져 있는 동안 MediaProjection 미러에는 surface가 없어 아무것도 캡처하지 않고, 동의는 남겨 두어 R29는 한 번만 묻습니다(`CaptureService.kt`).
- Windows 오버레이: 오버레이는 클릭이 통과하는 창 하나이고, 켜고 끄기 버튼은 작업 영역 오른쪽 아래의 늘 위에 있는 작은 창입니다. 둘 다 캡처에서 빠집니다(`WDA_EXCLUDEFROMCAPTURE`). 버튼 페이지는 안에 들어 있어 화면 빌드가 필요 없고, 누르면 번역이 켜지고 꺼집니다(`app.rs`, `pipeline::toggle`). Windows Graphics Capture는 화면이 바뀔 때만 프레임을 보내므로, `wgc.rs`가 Android 캡처처럼 가만있는 화면을 다시 보냅니다.
- `app` 기능과 데스크톱 세션이 있어야 하는 Windows 전용 검사: `cargo test -p realtime-screen-translator --features app --lib -- detect wgc`. 그린 만화 페이지에 실제 모델을 돌리는 검사(모델을 한 번 받음): `cargo test --release -p realtime-screen-translator --features app --test real_model -- --ignored`.

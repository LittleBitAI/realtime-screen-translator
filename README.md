# realtime-screen-translator

윈도우와 안드로이드에서 일본어·영어 만화를 열고, 그림 속 말풍선에 한국어 번역을 띄우는 앱입니다. 만화 회차 주소로 열거나 기기에 있는 그림 폴더를 가져와 읽을 수 있습니다.

## 앱 설치하기

### Windows

1. 설치 파일을 전달받은 작업 폴더에서 `artifacts/windows-installer/realtime-screen-translator_0.1.0_x64-setup.exe`를 찾으세요. 이 파일은 64비트 Windows용입니다. GitHub에서 소스만 받은 경우 [Windows 설치 파일 빌드](#windows-설치-파일-빌드)를 참고하세요.
2. 설치 파일을 실행하세요. 디지털 서명이 없는 설치 파일이라 Windows에서 경고가 표시될 수 있습니다.
3. 설치 창의 안내에 따라 설치를 마치세요.
4. 시작 메뉴에서 `realtime-screen-translator`를 여세요.

### Android

1. 현재 버전의 APK를 준비하세요. Android 8.0 이상인 ARM64 기기용으로, 갤럭시 S21 계열에서 사용할 수 있습니다. APK를 받은 경우 그 파일을 사용하세요. 직접 만들려면 [Android APK 빌드](#android-apk-빌드)를 참고하세요.
2. APK를 휴대폰으로 옮기세요.
3. 휴대폰에서 APK를 여세요.
4. 설치 권한을 묻는 경우, APK를 연 앱에 이 출처의 앱 설치를 허용하세요.
5. 설치 화면에서 `설치`를 누르세요.
6. 앱 목록에서 `실시간 화면 번역`을 여세요.

[GitHub Releases](https://github.com/LittleBitAI/realtime-screen-translator/releases)에 올라온 Android 시험판은 이전 버전입니다. 아래 폴더 가져오기와 목록 기능을 사용하려면 현재 버전의 APK가 필요합니다.

## 첫 회차 번역해서 읽기

1. [Google AI Studio](https://aistudio.google.com/apikey)에서 Gemini API 키를 발급받으세요.
2. 앱에서 `설정`을 여세요.
3. `Gemini API 키`의 `새 키`에 발급받은 키를 붙여 넣으세요.
4. `저장`을 누르세요.
5. `원문 언어`에서 읽을 만화에 맞춰 `일본어` 또는 `영어`를 고르세요.
6. `만화 열기`로 이동하세요.
7. `주소`에 읽을 회차의 웹 주소를 붙여 넣으세요.
8. `열기`를 누르세요.

Windows에서는 새 창에, Android에서는 앱 화면 안에 만화가 열립니다. 페이지 그림이 뜨면 말풍선에 한국어 번역이 자동으로 표시됩니다. 화면 아래의 `번역 켜짐`을 누르면 번역을 숨기고 새 번역 요청을 멈춥니다. `번역 꺼짐`을 누르면 다시 켭니다. `앱으로 돌아가기`를 누르면 앱 메뉴로 돌아갑니다.

## 그림 폴더를 한 회차로 읽기

1. `만화 열기`로 이동하세요.
2. `그림 폴더 가져오기`를 누르세요.
3. 읽을 그림이 들어 있는 폴더를 선택하세요.
4. Android에서는 `이 폴더 사용`을 누르세요.
5. Android에서 접근 권한을 묻는 창이 뜨면 `허용`을 누르세요.
6. 자동으로 열린 `번역본`의 `전체 작품`에서 폴더 이름과 같은 제목을 찾으세요.
7. 그 회차의 `읽기`를 누르세요.

선택한 폴더 바로 안의 JPG, JPEG, PNG, WebP 파일이 한 회차로 묶입니다. 파일 이름의 숫자는 크기순으로 정렬됩니다. 예를 들어 `1.jpg`, `2.jpg`, `10.jpg` 순으로 나옵니다.

앱은 그림을 기기 내 앱 저장 공간에 복사합니다. 원본과 복사본이 각각 공간을 차지합니다. 가져온 뒤 원본 폴더를 옮기거나 그림을 수정해도 앱은 복사해 둔 그림을 그대로 보여 줍니다. 수정본을 읽으려면 폴더를 다시 가져오세요.

## 저장한 회차 다시 열기

웹에서 새로 번역해 저장된 그림은 회차 주소별로 `번역본`에 모입니다. 그림 폴더는 가져오는 즉시 `전체 작품`에 들어갑니다. 웹 주소만 미리 저장하거나 원하는 제목을 붙이려면 다음 순서로 진행하세요.

1. `번역본`을 여세요.
2. `링크 추가`를 누르세요.
3. `작품 제목`에 기억하기 쉬운 제목을 적으세요.
4. `사이트 주소`에 회차 주소를 붙여 넣으세요.
5. `링크 저장`을 누르세요.

다시 읽을 때는 `번역본`에서 `전체 작품` 또는 담아 둔 목록을 여세요. `제목 또는 주소 검색`에 제목이나 주소 일부를 입력하세요. 찾은 회차에서 `읽기`를 누르세요. 웹 회차는 저장된 주소로 열리고, 가져온 회차는 기기에 복사한 그림으로 열립니다.

### 읽을 목록에 담기

1. `번역본`에서 `전체 작품`을 여세요. 이미 펼쳐져 있으면 그대로 두세요.
2. `편집`을 누르세요.
3. `새 목록 이름`을 입력하세요.
4. `목록 만들기`를 누르세요.
5. 담을 회차의 `작품을 담을 목록`에서 만든 목록에 체크하세요.
6. `편집 닫기`를 누르세요.

한 회차를 여러 목록에 담을 수 있습니다. 목록을 연 뒤 검색어를 비우면 `편집`에서 `작품 위로`와 `작품 아래로`로 읽을 순서를 바꿀 수 있습니다. 회차의 목록 체크를 해제하거나 목록 자체를 삭제해도 `전체 작품`의 제목, 저장한 링크, 그림과 번역은 그대로 남습니다.

### 그림 파일로 내보내기

1. `번역본`에서 그림이 있는 회차를 찾으세요.
2. `그림 내보내기`를 여세요.
3. `파일 형식`에서 `PNG` 또는 `WebP`를 고르세요.
4. `내보내기`를 누르세요.

저장된 번역이 있는 그림에는 한국어가 들어갑니다. 가져온 그림 중 아직 번역하지 않은 페이지는 원문이 보이는 상태로 내보냅니다.

Windows에서는 내려받기 폴더를 확인하세요. Android에서는 갤러리를 확인하세요. Android 10 이상에서는 `Pictures/RealtimeTranslator`에 저장됩니다.

## Gemini 요청과 번역 재사용

번역을 켜 두면 현재 페이지와 화면 가까이에 있는 페이지의 그림을 미리 번역합니다. 처음 보는 그림을 Gemini에 보내면 API 키가 연결된 Google 프로젝트의 요금제에 따라 비용이 생길 수 있습니다. 응답을 받지 못하면 요청을 다시 보낼 수 있습니다. 요청 횟수는 `만화 열기`의 `앱을 연 뒤 Gemini 요청`에서 확인하세요. [Gemini API 요금 안내](https://ai.google.dev/gemini-api/docs/pricing?hl=ko)

같은 기기에 저장된 번역은 그림 파일의 내용과 `원문 언어`가 모두 같을 때 재사용합니다. 페이지를 되돌아가거나 회차를 다시 열 때도 이 조건이 맞으면 Gemini에 다시 요청하지 않습니다. 그림 파일이 바뀌거나 원문 언어를 바꾸면 새 요청이 나갈 수 있습니다. 목록 정리와 내보내기는 Gemini를 호출하지 않습니다.

## 브라우저에서 할 수 있는 일

브라우저판에서는 `설정`과 `번역본`을 사용할 수 있습니다. 링크를 저장하고, 작품을 검색하고, 목록을 정리하고, 브라우저판에 저장된 그림을 내보낼 수 있습니다. 내보낸 뒤 `그림 1 내려받기`처럼 그림마다 표시되는 버튼을 누르세요. 웹 회차 열기, 그림 폴더 가져오기, 회차 읽기와 번역은 Windows 또는 Android에 설치한 앱에서 하세요.

## 개발 환경

Git, Rust(stable), C/C++ 빌드 도구, Node.js와 Corepack이 필요합니다. 테스트를 실행하려면 `cargo-nextest`도 설치하세요. Corepack은 `package.json`에 고정된 pnpm 버전을 사용합니다.

### Windows 준비

다음 명령은 PowerShell 7(`pwsh`)에서 실행하세요.

```powershell
winget install --id Microsoft.PowerShell
winget install --id Git.Git
winget install --id Rustlang.Rustup
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id OpenJS.NodeJS.LTS
```

설치가 끝나면 새 `pwsh`를 여세요.

```powershell
npm install --global corepack@latest
cargo install cargo-nextest --locked
git clone https://github.com/LittleBitAI/realtime-screen-translator.git
cd realtime-screen-translator
corepack pnpm install --frozen-lockfile
mkdir artifacts
```

### macOS 준비

[Homebrew](https://brew.sh)가 설치된 터미널에서 실행하세요.

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

### 브라우저판 실행

```sh
node scripts/live.mjs
```

`listening on http://localhost:8080`이 나오면 [브라우저판](http://localhost:8080)을 여세요. 첫 실행은 Rust 빌드 때문에 몇 분 걸릴 수 있습니다. 종료할 때는 `Ctrl+C`를 누르세요.

포트와 데이터 폴더는 환경 변수 `PORT`와 `APP_DATA_DIR`로 바꿀 수 있습니다. 기본값은 포트 8080과 시스템 임시 폴더의 `rst-web`입니다. 설치한 앱과는 별도의 데이터 폴더를 사용합니다.

### Windows 설치 파일 빌드

Windows 개발 환경을 준비한 뒤 저장소 루트에서 실행하세요.

```powershell
cargo install tauri-cli --version "^2" --locked
cargo tauri build --bundles nsis --features app --ci -c '{"build":{"beforeBuildCommand":"corepack pnpm build"}}'
```

완성된 파일은 `target/release/bundle/nsis/realtime-screen-translator_0.1.0_x64-setup.exe`에 있습니다.

### Android APK 빌드

Windows 개발 환경에 [Tauri의 Android 빌드 도구](https://v2.tauri.app/start/prerequisites/#android)를 설치하세요. 이 저장소는 JDK 17, Android SDK 플랫폼 36과 37.0, NDK `27.2.12479018`을 사용합니다. `JAVA_HOME`, `ANDROID_HOME`, `NDK_HOME`을 설치 경로에 맞추세요.

저장소 루트에서 설치 가능한 ARM64 디버그 APK를 빌드하세요.

```powershell
cargo install tauri-cli --version "^2" --locked
rustup target add aarch64-linux-android
cargo tauri android build --debug --apk --target aarch64 --features app --ci -c '{"build":{"beforeBuildCommand":"corepack pnpm build"}}'
```

Rust 빌드는 끝났지만 Windows에서 심볼릭 링크를 만들 수 없다는 오류가 나오면, 저장소 루트에서 아래 명령을 실행하세요.

```powershell
$jni = 'src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a'
New-Item -ItemType Directory -Force -Path $jni
Copy-Item 'target/aarch64-linux-android/debug/librealtime_screen_translator.so' -Destination $jni
Push-Location src-tauri/gen/android
.\gradlew.bat assembleUniversalDebug -x rustBuildUniversalDebug -x rustBuildArm64Debug -PabiList=arm64-v8a -ParchList=arm64 -PtargetList=aarch64
Pop-Location
```

완성된 파일은 `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`에 있습니다.

### 테스트

```sh
cargo run -q -p xtask -- artifacts/junit.xml && corepack pnpm install --frozen-lockfile && corepack pnpm test && corepack pnpm build
```

Rust와 화면 테스트 보고서는 `artifacts/`에 저장됩니다. 백엔드와 API는 [백엔드 설계](docs/design/backend.md)와 [API 명세](docs/design/api.yaml)에서, 앱 화면은 [화면 설계](docs/design/frontend.md)에서 확인할 수 있습니다.

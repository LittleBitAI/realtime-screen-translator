# realtime-screen-translator

화면을 실시간으로 인식해 번역하는 프로그램을 모바일과 PC 둘 다에서 쓸 수 있게 웹 브라우저에서 돌아가는 형태로 만듭니다. 가장 많이 쓰는 쪽은 모바일이라 갤럭시 S21을 기준 기기로 삼아 모바일 최적화를 가장 우선합니다. 대표 용도는 일본 만화를 한국어로 실시간 번역하는 일이며, 위아래 스크롤과 앞뒤 슬라이딩으로 이전/다음 페이지를 넘기는 방식을 구분해 트래킹하고, 이미 했던 번역과 효과음은 번역하지 않습니다. 번역 모델은 Gemini 3.1 Flash Lite이고, 기본 안전 가드레일은 끄고 프리필까지 넣습니다.

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

## Development

- Backend: `src-tauri/` (Rust). The commands of `docs/design/api.yaml` live in `src/backend.rs`; the Tauri shell is `src/app.rs` behind the `app` feature (`cargo tauri dev --features app`).
- Capture loop: `src/pipeline.rs` drives P6, P7 and P9 from captured frames. On Windows `src/wgc.rs` captures with Windows Graphics Capture and runs the balloon model with tract; on Android the Kotlin plugin in `src-tauri/plugins/screen/android/` captures with MediaProjection, runs the model with ONNX Runtime, draws the overlay and the floating on/off button, and seals the Gemini key with the Android Keystore.
- Balloon model: put a YOLOv8 detection export named `balloons.onnx` (contract in `src/detect.rs`: input `[1, 3, 640, 640]`, class 0 dialogue text, class 1 sound-effect text) in the app data folder under `models/` (Windows: `RST_DETECTOR_MODEL` overrides the path; Android: also read from the APK assets). Without it P1 shows `detector_missing`. `tests/fixtures/one-balloon.onnx` is only a contract stand-in for the tests.
- Windows-only checks that need the `app` feature and a desktop session: `cargo test -p realtime-screen-translator --features app --lib -- detect wgc`.
- Tests: install [cargo-nextest](https://nexte.st), then `cargo run -q -p xtask -- artifacts/junit.xml` runs every test and writes a JUnit report. The contract tests drive the backend through the `contract-host` binary.

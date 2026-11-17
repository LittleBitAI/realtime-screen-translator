//! Translation is never reported on where nothing can be captured: a host
//! that cannot capture other apps' screens (the browser host, a desktop other
//! than Windows) refuses P4 with `capture_unsupported`, and P1 says so.

use realtime_screen_translator::api::{FrameIn, KeyIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Host {
    can_capture: bool,
    granted: Permissions,
    starts: AtomicUsize,
    dir: PathBuf,
}

impl Platform for Host {
    fn request_permissions(&self) -> Permissions {
        self.granted
    }
    fn permissions(&self) -> Permissions {
        self.granted
    }
    fn can_capture(&self) -> bool {
        self.can_capture
    }
    fn start(&self) -> Result<(), String> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn stop(&self) {}
    fn emit(&self, _: &str, _: serde_json::Value) {}
    fn export_dir(&self) -> PathBuf {
        self.dir.join("export")
    }
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.to_vec())
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.to_vec())
    }
}

fn backend(name: &str, can_capture: bool, capture: bool, overlay: bool) -> (Backend, Arc<Host>) {
    let dir = std::env::temp_dir().join(format!("rst-capability-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let host = Arc::new(Host { can_capture, granted: Permissions { capture, overlay }, starts: AtomicUsize::new(0), dir: dir.clone() });
    let b = Backend::open(dir, None, host.clone()).unwrap();
    b.put_key(KeyIn { key: "test_key_ABCDEFGHIJKLMNOP".into() }).unwrap();
    (b, host)
}

#[test]
fn start_opens_a_session_only_where_capture_is_possible_and_granted() {
    // (host can capture, capture granted, overlay granted) → what P4 answers.
    let table = [
        (false, false, false, Err("capture_unsupported")),
        (false, true, true, Err("capture_unsupported")),
        (false, true, false, Err("capture_unsupported")),
        (true, false, false, Err("permission_required")),
        (true, true, false, Err("permission_required")),
        (true, false, true, Err("permission_required")),
        (true, true, true, Ok(())),
    ];
    for (i, (can_capture, capture, overlay, want)) in table.into_iter().enumerate() {
        let (b, host) = backend(&format!("row{i}"), can_capture, capture, overlay);
        let got = b.start().map(|_| ()).map_err(|e| e.0);
        let row = format!("row {i}: can_capture={can_capture} capture={capture} overlay={overlay}");
        assert_eq!(got, want, "{row}");
        let status = b.status();
        assert_eq!(status.capture_supported, can_capture, "{row}: P1 capture_supported");
        assert_eq!(status.on, want.is_ok(), "{row}: P1 on");
        // The device is only asked to start capture where it can and may.
        assert_eq!(host.starts.load(Ordering::SeqCst), usize::from(want.is_ok()), "{row}: device starts");
        if want.is_err() {
            // No session: frames are refused, nothing is saved.
            let frame = FrameIn { seq: 1, width: 96, height: 96, thumb: vec![200; 96 * 96], captured_at: 0 };
            assert_eq!(b.frame(frame).map(|_| ()).map_err(|e| e.0), Err("not_on"), "{row}: frames after a refused start");
        }
    }
}

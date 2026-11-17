//! The database flushes to disk at checkpoints, not at every commit (R7: a
//! frame's writes do not wait on the disk), and what was committed is still
//! there when the app opens it again.

mod support;

use realtime_screen_translator::store;
use support::Dirs;

#[test]
fn commits_do_not_wait_on_the_disk_and_survive_reopening() {
    let dirs = Dirs::new();
    let db = store::open(&dirs.data, 1).unwrap();
    let sync: i64 = db.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
    assert_eq!(sync, 1, "synchronous is not NORMAL");
    let mode: String = db.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    store::put_setting(&db, "probe", b"kept", 1).unwrap();
    drop(db);
    let db = store::open(&dirs.data, 2).unwrap();
    assert_eq!(store::setting(&db, "probe").unwrap().as_deref(), Some(&b"kept"[..]));
    std::mem::forget(dirs);
}

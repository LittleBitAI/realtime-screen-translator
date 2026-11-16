//! R25: the balloon model ships with the app as a pinned download. On first
//! start the app fetches the Apache-2.0 "comic-text-and-bubble-detector"
//! (RT-DETR-v2, int8, 11 MB) from a fixed revision, checks its SHA-256 and
//! keeps it in the app data folder; detection uses it from then on, offline.
//! Until the file is in place P1 shows `detector_missing`.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const FILE: &str = "balloons.onnx";
pub const URL: &str = "https://huggingface.co/ogkalu/comic-text-and-bubble-detector/resolve/16e8a622f91fabc6b5b65c96d32d1183f8843546/detector-v4-s_int8.onnx";
pub const SHA256: &str = "5fe9e4f576e49d4e7e8b0e029d6d3cdc252abd4694113e1cae120e62c931ea79";
const MAX_BYTES: u64 = 64 << 20;

/// Where the model lives (`RST_DETECTOR_MODEL` overrides it).
pub fn path(data_dir: &Path) -> PathBuf {
    std::env::var_os("RST_DETECTOR_MODEL").map(PathBuf::from).unwrap_or_else(|| data_dir.join("models").join(FILE))
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The model at `path`, fetched from `url` if it is missing or not the
/// pinned file. Nothing is kept unless its SHA-256 is `sha`.
pub fn ensure(agent: &ureq::Agent, url: &str, sha: &str, path: &Path) -> Result<(), String> {
    if std::fs::read(path).is_ok_and(|b| sha256(&b) == sha) {
        return Ok(());
    }
    let mut resp = agent.get(url).call().map_err(|e| format!("model download: {e}"))?;
    if resp.status() != 200 {
        return Err(format!("model download: HTTP {}", resp.status()));
    }
    let bytes = resp.body_mut().with_config().limit(MAX_BYTES).read_to_vec().map_err(|e| format!("model download: {e}"))?;
    if sha256(&bytes) != sha {
        return Err("model download: checksum mismatch".into());
    }
    let dir = path.parent().ok_or("model path has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Written aside and renamed, so a reader never sees half a model.
    let part = path.with_extension("part");
    std::fs::write(&part, &bytes).and_then(|_| std::fs::rename(&part, path)).map_err(|e| e.to_string())
}

/// Keep trying until the model is in place (first start may be offline).
pub fn fetch_in_background(path: PathBuf) {
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(600)))
            .proxy(ureq::Proxy::try_from_env())
            .build()
            .into();
        let mut wait = Duration::from_secs(10);
        while let Err(e) = ensure(&agent, URL, SHA256, &path) {
            eprintln!("{e}; trying again in {} s", wait.as_secs());
            std::thread::sleep(wait);
            wait = (wait * 2).min(Duration::from_secs(600));
        }
    });
}

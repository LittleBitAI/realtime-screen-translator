//! `cargo run -q -p xtask -- <report.xml>`: run the workspace tests with
//! cargo-nextest and write its JUnit report to the given path, whatever the
//! outcome, adding each testcase's source `file` (relative to the repository
//! root). Exits with nextest's status.

use std::path::{Path, PathBuf};
use std::process::{exit, Command};

fn main() {
    let report = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: cargo run -p xtask -- <junit-report.xml>");
        exit(2)
    });
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["nextest", "run", "--workspace", "--exclude", "xtask", "--profile", "ci", "--no-fail-fast"])
        .current_dir(&root)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("cannot run cargo nextest: {e}");
            exit(2)
        });
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    let junit = target.join("nextest").join("ci").join("junit.xml");
    match std::fs::read_to_string(&junit) {
        Ok(xml) => {
            if let Err(e) = std::fs::write(&report, with_files(&xml, &root)) {
                eprintln!("cannot write {report}: {e}");
                exit(2);
            }
        }
        Err(e) => eprintln!("nextest wrote no JUnit report at {}: {e}", junit.display()),
    }
    exit(status.code().unwrap_or(1));
}

/// `<testsuite name="pkg::target">` of an integration test target maps to
/// `<member>/tests/<target>.rs`; give each of its testcases that `file`.
fn with_files(xml: &str, root: &Path) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut file: Option<String> = None;
    let mut rest = xml;
    loop {
        let suite = rest.find("<testsuite ");
        let case = rest.find("<testcase ");
        let Some(i) = [suite, case].into_iter().flatten().min() else { break };
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if Some(i) == suite {
            file = attr(rest, "name").and_then(|n| test_file(&n, root));
            out.push_str("<testsuite ");
            rest = &rest["<testsuite ".len()..];
        } else {
            out.push_str("<testcase ");
            if let Some(f) = &file {
                out.push_str(&format!("file=\"{f}\" "));
            }
            rest = &rest["<testcase ".len()..];
        }
    }
    out.push_str(rest);
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let end = tag.find('>')?;
    let key = format!("{name}=\"");
    let start = tag[..end].find(&key)? + key.len();
    let len = tag[start..end].find('"')?;
    Some(tag[start..start + len].to_string())
}

fn test_file(suite: &str, root: &Path) -> Option<String> {
    let (_, target) = suite.split_once("::")?;
    if target.contains('/') {
        return None;
    }
    std::fs::read_dir(root).ok()?.flatten().find_map(|e| {
        let rel = format!("{}/tests/{target}.rs", e.file_name().to_string_lossy());
        root.join(&rel).is_file().then_some(rel)
    })
}

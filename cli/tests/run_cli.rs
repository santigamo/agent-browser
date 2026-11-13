#![cfg(all(unix, feature = "codemode"))]

//! `agent-browser run` end to end for scripts that need no browser: how its output is capped,
//! wrapped and reported.

use serde_json::Value;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

struct Fixture(TempDir);

impl Fixture {
    fn new() -> Self {
        let fixture = Self(tempfile::tempdir().unwrap());
        std::fs::create_dir(fixture.0.path().join("s")).unwrap();
        fixture
    }

    fn run(&self, args: &[&str], script: &str) -> Output {
        let root = self.0.path();
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent-browser"))
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", root)
            .env("USERPROFILE", root)
            .env("TMPDIR", root)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("AGENT_BROWSER_SOCKET_DIR", root.join("s"))
            .env("NO_COLOR", "1")
            .current_dir(root)
            .args(["--session", "run-test"])
            .args(args)
            .args(["run", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(pid) = std::fs::read_to_string(self.0.path().join("s/run-test.pid")) {
            if let Ok(pid) = pid.trim().parse::<i32>() {
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
        }
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

#[test]
fn a_returned_value_is_printed_and_piped_output_is_not_capped_by_default() {
    let f = Fixture::new();
    let out = f.run(&[], "return 'x'.repeat(20000);");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(stdout(&out).trim_end().len(), 20_000);
}

#[test]
fn max_output_caps_the_returned_text_with_head_and_tail() {
    let f = Fixture::new();
    let out = f.run(
        &["--max-output", "100"],
        "return 'HEAD' + 'x'.repeat(5000) + 'TAIL';",
    );
    assert!(out.status.success(), "{out:?}");
    let text = stdout(&out);
    assert!(
        text.starts_with("HEAD") && text.trim_end().ends_with("TAIL"),
        "{text}"
    );
    assert!(text.contains("omitted"), "{text}");
    assert!(text.len() < 400, "{}", text.len());
}

#[test]
fn content_boundaries_wrap_the_output() {
    let f = Fixture::new();
    let out = f.run(&["--content-boundaries"], "return 'hello';");
    assert!(out.status.success(), "{out:?}");
    let text = stdout(&out);
    assert!(
        text.starts_with("--- AGENT_BROWSER_PAGE_CONTENT nonce="),
        "{text}"
    );
    assert!(
        text.contains("\nhello\n") && text.contains("--- END_AGENT_BROWSER_PAGE_CONTENT"),
        "{text}"
    );
}

#[test]
fn json_uses_the_usual_success_data_error_envelope() {
    let f = Fixture::new();
    let ok = f.run(&["--json"], "return 'hello';");
    assert!(ok.status.success(), "{ok:?}");
    let v: Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(v["success"], true);
    assert_eq!(v["data"]["output"], "hello");
    assert!(v["error"].is_null());
    let bad = f.run(&["--json"], "throw new Error('boom');");
    assert_eq!(bad.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&bad.stdout).unwrap();
    assert_eq!(v["success"], false);
    assert!(v["error"].as_str().unwrap().contains("boom"), "{v}");
    assert!(v["data"].is_null());
}

#[test]
fn a_script_without_a_value_says_so_instead_of_ok() {
    let f = Fixture::new();
    let out = f.run(&[], "const x = 1;");
    assert!(out.status.success(), "{out:?}");
    assert!(
        stdout(&out).contains("(no value returned"),
        "{}",
        stdout(&out)
    );
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_a_script_drives_a_real_page_and_stops_at_the_failing_step() {
    let f = Fixture::new();
    let page = "data:text/html,<input id=q><button id=go onclick=\"document.getElementById('out').textContent='hi '+document.getElementById('q').value\">Go</button><p id=out></p>";
    let script = format!(
        "page.goto({page:?}); page.fill('#q', 'Ana'); page.click('#go'); \
         const out = page.text('#out'); page.click('#missing-button'); return out;"
    );
    let out = f.run(&[], &script);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        err.contains("step 5: page.click(\"#missing-button\") failed"),
        "{err}"
    );
    let ok = f.run(&[], &format!("page.goto({page:?}); page.fill('#q', 'Ana'); page.click('#go'); return page.text('#out');"));
    assert!(ok.status.success(), "{ok:?}");
    assert_eq!(stdout(&ok).trim(), "hi Ana");
    let _ = f.run_close();
}

impl Fixture {
    fn run_close(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_agent-browser"))
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.0.path())
            .env("AGENT_BROWSER_SOCKET_DIR", self.0.path().join("s"))
            .args(["--session", "run-test", "close"])
            .output()
            .unwrap()
    }
}

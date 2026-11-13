// Added in the browser-duel fork of vercel-labs/agent-browser v0.43.0 (Apache-2.0); see CHANGES-FORK.md.
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
        "page.goto({page:?}); page.fill('#q', 'Pat'); page.click('#go'); \
         const out = page.text('#out'); page.click('#missing-button'); return out;"
    );
    let out = f.run(&[], &script);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        err.contains("step 5: page.click(\"#missing-button\") failed"),
        "{err}"
    );
    let ok = f.run(&[], &format!("page.goto({page:?}); page.fill('#q', 'Pat'); page.click('#go'); return page.text('#out');"));
    assert!(ok.status.success(), "{ok:?}");
    assert_eq!(stdout(&ok).trim(), "hi Pat");
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

// ---------------------------------------------------------------------------
// The locator layer against a real DOM (Chrome-gated)
// ---------------------------------------------------------------------------

const DOM: &str = r##"<!doctype html><html><head><meta charset="utf-8"><title>Locators</title></head><body>
<h1>Catalog</h1>
<ul id="items">
  <li class="x" style="display:none">Hidden template one</li>
  <li class="x" style="display:none">Hidden template two</li>
  <li class="x">Visible alpha</li>
  <li class="x">Visible beta <a href="#b" class="more">More</a></li>
  <li class="x">Visible gamma <a href="#c" class="more">More</a></li>
</ul>
<form>
  <label for="name">Full name</label><input id="name" placeholder="Your name">
  <label for="mail">Contact email</label><input id="mail" type="email" placeholder="you@example.test">
  <button type="button" id="save" onclick="window.saved = (window.saved || 0) + 1">Save changes</button>
  <button type="button" id="gone" style="display:none">Secret action</button>
  <input type="checkbox" id="tos" style="position:absolute;opacity:0;width:0;height:0">
  <label for="tos" id="toslabel" style="display:inline-block;padding:6px">Accept the terms</label>
</form>
<p class="quote">He said "a >> b" quietly</p>
<div id="wrap"><section><p>Deep paragraph with unique phrase here</p></section></div>
<my-card></my-card>
<script>
customElements.define('my-card', class extends HTMLElement { constructor() { super();
  const r = this.attachShadow({ mode: 'open' });
  r.innerHTML = '<h2>Card title</h2><button id="inner" onclick="this.dataset.hit=1">Card action</button>'; } });
</script></body></html>"##;

impl Fixture {
    /// Opens `html` in the session and runs `body` (a script) on it.
    fn on_page(&self, html: &str, body: &str) -> Output {
        let script = format!(
            "page.goto('data:text/html;charset=utf-8,' + encodeURIComponent({html:?})); {body}"
        );
        self.run(&[], &script)
    }

    fn value(&self, html: &str, body: &str) -> Value {
        let out = self.on_page(html, body);
        assert!(
            out.status.success(),
            "stdout: {} stderr: {}",
            stdout(&out),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_str(stdout(&out).trim())
            .unwrap_or_else(|e| panic!("{e}: {}", stdout(&out)))
    }

    fn failure(&self, html: &str, body: &str) -> String {
        let out = self.on_page(html, body);
        assert_eq!(
            out.status.code(),
            Some(1),
            "expected a failure: {}",
            stdout(&out)
        );
        String::from_utf8_lossy(&out.stderr).to_string()
    }
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_locators_find_elements_by_role_text_label_placeholder_and_chains() {
    let f = Fixture::new();
    let v = f.value(
        DOM,
        r#"return {
          role: page.getByRole('button', {name: 'Save changes'}).text(),
          roleRe: page.getByRole('button', {name: /save/i}).count(),
          heading: page.getByRole('heading', {name: 'Catalog'}).text(),
          text: page.getByText('Visible alpha').text(),
          label: page.getByLabel('Contact email').getAttribute('type'),
          placeholder: page.getByPlaceholder('Your name').getAttribute('id'),
          css: page.locator('#items li.x').count(),
          engine: page.locator('text=Visible gamma').count(),
          chain: page.locator('li.x >> a.more').count(),
          nth: page.locator('a.more').nth(1).getAttribute('href'),
          last: page.locator('a.more').last().getAttribute('href'),
          has: page.locator('li.x').filter({hasText: 'gamma'}).locator('a.more').getAttribute('href'),
          hasText: page.locator('li:has-text("beta") .more').getAttribute('href'),
          inShadow: page.getByText('Card title').text(),
          shadowRole: page.getByRole('button', {name: 'Card action'}).text(),
        };"#,
    );
    assert_eq!(v["role"], "Save changes");
    assert_eq!(v["roleRe"], 1);
    assert_eq!(v["heading"], "Catalog");
    assert_eq!(v["text"], "Visible alpha");
    assert_eq!(v["label"], "email");
    assert_eq!(v["placeholder"], "name");
    assert_eq!(
        v["css"], 3,
        "hidden templates are not counted when some match is visible"
    );
    assert_eq!(v["engine"], 1);
    assert_eq!(v["chain"], 2);
    assert_eq!(v["nth"], "#c");
    assert_eq!(v["last"], "#c");
    assert_eq!(v["has"], "#c");
    assert_eq!(v["hasText"], "#b");
    assert_eq!(v["inShadow"], "Card title");
    assert_eq!(v["shadowRole"], "Card action");
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_locator_actions_act_on_the_visible_match_and_leave_no_marker_behind() {
    let f = Fixture::new();
    let v = f.value(
        DOM,
        r#"page.getByLabel('Full name').fill('Pat');
           page.getByRole('button', {name: 'Save changes'}).click();
           page.getByRole('button', {name: 'Card action'}).click();
           page.getByLabel('Accept the terms').check();
           return {
             name: page.value('#name'),
             saved: page.evaluate(() => window.saved),
             inner: page.evaluate(() => document.querySelector('my-card').shadowRoot.getElementById('inner').dataset.hit),
             tos: page.evaluate(() => document.getElementById('tos').checked),
           };"#,
    );
    assert_eq!(v["name"], "Pat");
    assert_eq!(v["saved"], 1);
    assert_eq!(v["inner"], "1");
    assert_eq!(v["tos"], true);
    // the markers a run puts in the page are removed when it ends (a second run sees none)
    let again = f.run(
        &[],
        "return page.evaluate(() => document.documentElement.outerHTML.includes('data-ab-loc'));",
    );
    assert_eq!(stdout(&again).trim(), "false");
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_is_visible_is_false_for_no_match_and_for_a_hidden_match() {
    let f = Fixture::new();
    let v = f.value(
        DOM,
        r#"return {
          none: page.getByText('No such text anywhere').isVisible(),
          noneCss: page.locator('.not-here').isVisible(),
          hidden: page.getByRole('button', {name: 'Secret action'}).isVisible(),
          shown: page.getByRole('button', {name: 'Save changes'}).isVisible(),
        };"#,
    );
    assert_eq!(v["none"], false);
    assert_eq!(v["noneCss"], false);
    assert_eq!(v["hidden"], false);
    assert_eq!(v["shown"], true);
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_locator_waits_pass_through_state_and_timeout_or_throw_on_an_unsupported_state() {
    let f = Fixture::new();
    let v = f.value(
        DOM,
        r#"const t0 = Date.now();
           page.getByText('Visible alpha').waitFor({state: 'visible', timeout: 2000});
           page.getByRole('button', {name: 'Secret action'}).waitFor({state: 'hidden', timeout: 2000});
           page.locator('#gone').waitFor({state: 'attached', timeout: 2000});
           page.getByText('No such text anywhere').waitFor({state: 'detached', timeout: 2000});
           return {fast: Date.now() - t0 < 6000};"#,
    );
    assert_eq!(v["fast"], true);
    // a visible wait that cannot be satisfied honors the timeout it was given
    let started = std::time::Instant::now();
    let err = f.failure(
        DOM,
        "page.getByText('No such text anywhere').waitFor({state: 'visible', timeout: 700});",
    );
    assert!(
        err.contains("Timed out after 700ms waiting for text=No such text anywhere to be visible"),
        "{err}"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    let err = f.failure(
        DOM,
        "page.getByText('Visible alpha').waitFor({state: 'ready'});",
    );
    assert!(err.contains("unsupported state 'ready'"), "{err}");
    let err = f.failure(
        DOM,
        "page.getByText('Visible alpha').waitFor({strict: true});",
    );
    assert!(err.contains("unsupported option 'strict'"), "{err}");
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_a_locator_that_cannot_be_evaluated_is_an_error_not_zero_matches() {
    let f = Fixture::new();
    let err = f.failure(DOM, "page.locator('text=/(/').click();");
    assert!(
        err.contains("Invalid locator text=/(/") && !err.contains("Element not found"),
        "{err}"
    );
    let err = f.failure(DOM, "page.locator('li[').count();");
    assert!(err.contains("Invalid locator"), "{err}");
    let err = f.failure(DOM, "page.getByText('x').locator('css=div[[').isVisible();");
    assert!(err.contains("Invalid locator"), "{err}");
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_a_marker_that_disappears_is_resolved_once_more_and_errors_name_the_original_locator() {
    // The page swaps the tagged button for a fresh node as soon as it is marked, once: the click
    // that follows finds no marker, resolves the locator again and succeeds.
    let swap = r##"<!doctype html><body><button id="b" onclick="window.clicks=(window.clicks||0)+1">Rerender me</button>
<script>
let swapped = false;
new MutationObserver(() => {
  const b = document.getElementById('b');
  if (!swapped && b && b.hasAttribute('data-ab-loc')) {
    swapped = true;
    const n = b.cloneNode(true); n.removeAttribute('data-ab-loc'); b.replaceWith(n);
  }
}).observe(document.body, { attributes: true, subtree: true });
</script></body>"##;
    let f = Fixture::new();
    let v = f.value(
        swap,
        "page.getByRole('button', {name: 'Rerender me'}).click(); return page.evaluate(() => window.clicks);",
    );
    assert_eq!(v, 1);
    // An element that is removed for good: the error names the locator, never the marker.
    let vanish = r##"<!doctype html><body><button id="b">Vanishing</button>
<script>
new MutationObserver(() => {
  const b = document.getElementById('b');
  if (b && b.hasAttribute('data-ab-loc')) b.remove();
}).observe(document.body, { attributes: true, subtree: true });
</script></body>"##;
    let err = f.failure(
        vanish,
        "page.getByRole('button', {name: 'Vanishing'}).click();",
    );
    assert!(
        err.contains("Vanishing") && !err.contains("data-ab-loc"),
        "{err}"
    );
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_nth_count_and_the_ambiguity_note_index_the_same_visible_matches() {
    let f = Fixture::new();
    let out = f.on_page(
        DOM,
        r#"const l = page.locator('li.x');
           const r = {
             count: l.count(),
             first: l.first().text(),
             second: l.nth(1).text(),
             last: l.last().text(),
             all: l.all().map((x) => x.text()),
           };
           page.locator('li.x').click();
           return r;"#,
    );
    assert!(
        out.status.success(),
        "{} {}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    // the note counts the visible matches, with the hidden ones as the remainder
    assert!(
        text.contains("matched 3 visible elements (5 in all); acted on the first"),
        "{text}"
    );
    let json = &text[text.find('{').unwrap()..];
    let v: Value = serde_json::from_str(json).unwrap();
    assert_eq!(v["count"], 3);
    assert_eq!(v["first"], "Visible alpha");
    assert!(
        v["second"].as_str().unwrap().starts_with("Visible beta"),
        "{v}"
    );
    assert!(
        v["last"].as_str().unwrap().starts_with("Visible gamma"),
        "{v}"
    );
    assert_eq!(v["all"].as_array().unwrap().len(), 3);
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_unsupported_options_and_filters_throw_instead_of_being_ignored() {
    let f = Fixture::new();
    for (script, needle) in [
        (
            "page.getByRole('button', {name: 'Save changes'}).click({force: true});",
            "unsupported option 'force'",
        ),
        (
            "page.click('#save', {trial: true});",
            "unsupported option 'trial'",
        ),
        (
            "page.locator('li').filter({has: page.locator('a')}).count();",
            "unsupported option 'has'",
        ),
        (
            "page.getByRole('button', {name: 'x', checked: true}).count();",
            "unsupported option 'checked'",
        ),
        (
            "page.goto('about:blank', {waitUntil: 'later'});",
            "unsupported waitUntil 'later'",
        ),
        ("page.snapshot({find: /abc/});", "find takes a string"),
        (
            "page.snapshot({visibleOnly: true});",
            "unsupported option 'visibleOnly'",
        ),
        (
            "page.getByText('x', {ignoreCase: true}).count();",
            "unsupported option 'ignoreCase'",
        ),
    ] {
        let err = f.failure(DOM, script);
        assert!(err.contains(needle), "{script}: {err}");
    }
    // supported forms still work: timeout on an action, hasText, waitUntil on goto
    let v = f.value(
        DOM,
        "page.click('#save', {timeout: 3000}); page.goto('about:blank', {waitUntil: 'domcontentloaded'}); return 1;",
    );
    assert_eq!(v, 1);
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_a_bare_has_text_picks_the_deepest_element_and_quotes_protect_the_chain_operator() {
    let f = Fixture::new();
    let v = f.value(
        DOM,
        r#"return {
          count: page.locator(':has-text("unique phrase here")').count(),
          text: page.locator(':has-text("unique phrase here")').text(),
          quoted: page.locator('p.quote:has-text("a >> b")').count(),
          quotedChain: page.locator('p:has-text("a >> b") >> text=quietly').count(),
        };"#,
    );
    assert_eq!(
        v["count"], 1,
        "only the deepest element, not html/body/#wrap/section: {v}"
    );
    assert_eq!(v["text"], "Deep paragraph with unique phrase here");
    assert_eq!(v["quoted"], 1);
    assert_eq!(v["quotedChain"], 1);
}

#[test]
#[ignore = "requires Chrome"]
fn e2e_wait_for_and_the_action_that_follows_use_the_same_visible_match() {
    let page = r##"<!doctype html><body>
<button class="go" style="display:none" onclick="window.hit='hidden'">Go</button>
<button class="go" onclick="window.hit='visible'">Go</button></body>"##;
    let f = Fixture::new();
    let out = f.on_page(
        page,
        "const l = page.getByRole('button', {name: 'Go'}); l.waitFor({timeout: 2000}); l.click(); \
         return page.evaluate(() => window.hit);",
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout(&out).trim(), "visible");
}

// ---------------------------------------------------------------------------
// The MCP run tool
// ---------------------------------------------------------------------------

impl Fixture {
    /// `agent-browser mcp call agent_browser_run <arguments>` against this fixture's session.
    fn mcp_run(&self, mut arguments: Value) -> Value {
        arguments["session"] = Value::String("run-test".into());
        let root = self.0.path();
        let output = Command::new(env!("CARGO_BIN_EXE_agent-browser"))
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", root)
            .env("USERPROFILE", root)
            .env("TMPDIR", root)
            .env("AGENT_BROWSER_SOCKET_DIR", root.join("s"))
            .env("NO_COLOR", "1")
            .current_dir(root)
            .args(["mcp", "call", "agent_browser_run", &arguments.to_string()])
            .output()
            .unwrap();
        serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("{e}: {output:?}"))
    }
}

#[test]
fn the_mcp_run_tool_returns_the_script_text_alone_and_honors_a_requested_cap() {
    let f = Fixture::new();
    let result = f.mcp_run(serde_json::json!({ "code": "return 'hello ' + (6 * 7);" }));
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["content"][0]["text"], "hello 42");
    assert!(result.get("structuredContent").is_none(), "{result}");
    // whole by default, capped only when the caller passes --max-output in extraArgs
    let whole = f.mcp_run(serde_json::json!({ "code": "return 'z'.repeat(20000);" }));
    assert_eq!(whole["content"][0]["text"].as_str().unwrap().len(), 20_000);
    let capped = f.mcp_run(serde_json::json!({
        "code": "return 'HEAD' + 'z'.repeat(20000) + 'TAIL';",
        "extraArgs": ["--max-output", "200"],
    }));
    let text = capped["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("HEAD") && text.ends_with("TAIL") && text.len() < 600,
        "{}",
        text.len()
    );
    // a failing script is an error result with the step message as text
    let failed = f.mcp_run(serde_json::json!({ "code": "throw new Error('nope');" }));
    assert_eq!(failed["isError"], true, "{failed}");
    assert!(failed["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("nope"));
}

#[test]
fn the_mcp_timeout_bounds_the_script() {
    let f = Fixture::new();
    let started = std::time::Instant::now();
    let result =
        f.mcp_run(serde_json::json!({ "code": "sleep(60000); return 1;", "timeoutMs": 6000 }));
    assert_eq!(result["isError"], true, "{result}");
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("script timed out after 3000 ms"),
        "{result}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
}

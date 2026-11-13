//! Code mode: `agent-browser run <script.js|->` executes a JavaScript script with a
//! Playwright-like `page` handle. Every `page.*` call is turned into a normal agent-browser
//! command and sent through the same parser and daemon as a batch item, so refs (@e1), waits, the
//! domain allowlist, action policies and confirmations behave exactly as they do for single
//! commands. Only the script's return value (plus console.log lines) is printed, capped in size
//! like any page text.

use crate::commands;
use crate::connection::{DaemonOptions, Response};
use crate::flags::Flags;
use crate::output::{self, OutputOptions};
use rquickjs::{CatchResultExt, Context, Function, Promise, Runtime};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::io::Read as _;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The JavaScript half, in the order it is evaluated: log capture, selector parser, the resolver
/// (whose source runs in the page), target resolution, the page API and the locator objects.
const PRELUDE: &str = concat!(
    include_str!("js/log.js"),
    "\n",
    include_str!("js/selector.js"),
    "\n",
    include_str!("js/resolver.js"),
    "\n",
    include_str!("js/targets.js"),
    "\n",
    include_str!("js/page.js"),
    "\n",
    include_str!("js/locator.js"),
);
/// Bound on what `console.log` may collect inside the script (the printed output is capped
/// separately, like any page text).
const LOG_CAP_CHARS: usize = 1 << 20;
const DEFAULT_SCRIPT_TIMEOUT_MS: u64 = 120_000;

/// The CLI state a script run borrows.
struct Cli<'a> {
    flags: &'a Flags,
    daemon_opts: &'a DaemonOptions<'a>,
}

/// QuickJS stores the native functions of a script in its runtime and wants them `'static`, but
/// the CLI state they use is borrowed from `main`. This is the one place that bridges the two,
/// and it is sound because the loan can be revoked: the pointer sits in a shared slot that
/// `Loan` clears when `run_script_within` ends (on every path, by `Drop`), and a function that
/// still exists then finds the slot empty and fails the step instead of touching the borrow.
/// Everything runs on one thread, so the slot is never read while it is being cleared.
struct Lent {
    slot: Rc<Cell<*const Cli<'static>>>,
}

/// Clears the slot of a `Lent` when dropped.
struct Loan {
    slot: Rc<Cell<*const Cli<'static>>>,
}

impl Drop for Loan {
    fn drop(&mut self) {
        self.slot.set(std::ptr::null());
    }
}

impl Lent {
    /// Lends `cli` to JS functions until the returned `Loan` is dropped.
    fn lend<'a>(cli: &'a Cli<'a>) -> (Lent, Loan) {
        // Only the lifetime parameter changes in this cast, never the layout of the value. The
        // pointer is dereferenced only through `with`, only while the `Loan` (which borrows
        // nothing but is dropped before `cli`'s scope ends) has not cleared it.
        #[allow(clippy::unnecessary_cast)] // the cast is what erases the lifetime
        let ptr = cli as *const Cli<'a> as *const Cli<'static>;
        let slot = Rc::new(Cell::new(ptr));
        (Lent { slot: slot.clone() }, Loan { slot })
    }

    /// Runs `f` on the borrowed state, or returns None once the loan has ended.
    fn with<R>(&self, f: impl FnOnce(&Cli<'_>) -> R) -> Option<R> {
        let ptr = self.slot.get();
        // SAFETY: a non-null slot means the `Loan` is alive, so the `Cli` it was made from is
        // still borrowed by `run_script_within`; see the type documentation.
        unsafe { ptr.as_ref() }.map(f)
    }
}

/// What a script run tracks besides the borrowed CLI state.
struct Session {
    lent: Lent,
    relaunch_applied: bool,
    /// The script's hard stop and its configured length, for the clamps below.
    deadline: Instant,
    timeout_ms: u64,
}

/// A sleep never outlasts the script: it is cut to the time left, plus 1 ms so the deadline is really crossed
/// and the interrupt handler ends the script at its next instruction.
fn clamp_sleep(ms: f64, remaining: Duration) -> Duration {
    let want = Duration::from_millis(ms.clamp(0.0, 30_000.0) as u64);
    if want > remaining {
        remaining + Duration::from_millis(1)
    } else {
        want
    }
}

/// An operation timeout carried by a command (the wait family) is cut to the time the script has left.
fn clamp_op_timeout(cmd: &mut Value, remaining: Duration) {
    if let Some(t) = cmd.get("timeout").and_then(Value::as_u64) {
        cmd["timeout"] = json!(t.min(remaining.as_millis() as u64));
    }
}

/// Actions the daemon can settle after: a navigation they start within ~150 ms is awaited.
fn settles_after(action: Option<&str>) -> bool {
    matches!(action, Some("click") | Some("press"))
}

/// Runs one command line through the batch pipeline and returns the response as
/// `{success, data, error}` JSON.
fn exec_one(s: &mut Session, argv: Vec<String>) -> Value {
    let remaining = s.deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return json!({ "success": false, "error": format!("script timed out after {} ms", s.timeout_ms) });
    }
    let relaunch_applied = &mut s.relaunch_applied;
    let sent = s.lent.with(|cli| {
        let (mut parsed, action) =
            match crate::build_batch_command(cli.flags, &argv, *relaunch_applied) {
                Ok(built) => built,
                Err(message) => return json!({ "success": false, "error": message }),
            };
        clamp_op_timeout(&mut parsed, remaining);
        // Actions that can start a navigation wait for it (same settle as `--snapshot`), so the
        // next page.* call does not read the page being left.
        if settles_after(action.as_deref()) {
            parsed["settle"] = json!(true);
        }
        match crate::send_command_with_respawn(parsed, &cli.flags.session, cli.daemon_opts) {
            Ok(resp) => {
                if action.as_deref() == Some("close") && crate::close_completed(&resp) {
                    crate::clear_closed_session(&cli.flags.session);
                }
                *relaunch_applied |= crate::response_applied_relaunch(&resp);
                response_json(&resp)
            }
            Err(e) => json!({ "success": false, "error": e }),
        }
    });
    sent.unwrap_or_else(|| json!({ "success": false, "error": "the script has already ended" }))
}

/// The step's outcome as `{success, data, error}`. A gated action that the daemon parked for
/// confirmation did not run: the step fails and names the confirm id, instead of looking like it
/// succeeded.
fn response_json(resp: &Response) -> Value {
    if let Some(data) = resp
        .data
        .as_ref()
        .filter(|d| d["confirmation_required"] == true)
    {
        let id = data["confirmation_id"].as_str().unwrap_or("?");
        let action = data["action"].as_str().unwrap_or("this action");
        return json!({
            "success": false,
            "code": "confirmation_required",
            "error": format!(
                "{action} needs confirmation and was NOT run (confirm id {id}): approve it with \
                 `agent-browser confirm {id}` or refuse it with `agent-browser deny {id}`, then \
                 run the script again"
            ),
        });
    }
    let mut out = json!({ "success": resp.success, "data": resp.data });
    if !resp.success {
        let mut err = resp.error.clone().unwrap_or_else(|| "unknown error".into());
        if let Some(w) = &resp.warning {
            err.push_str(&format!(" ({w})"));
        }
        out["error"] = json!(err);
    }
    out
}

fn render_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

/// How a script ended: the text to print and whether it succeeded.
pub struct RunOutcome {
    pub text: String,
    pub ok: bool,
}

/// Runs the script with the configured time budget (`AGENT_BROWSER_RUN_TIMEOUT_MS`, default 120 s).
pub fn run_script(script: &str, flags: &Flags, daemon_opts: &DaemonOptions) -> RunOutcome {
    let timeout = std::env::var("AGENT_BROWSER_RUN_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SCRIPT_TIMEOUT_MS);
    run_script_within(script, flags, daemon_opts, timeout)
}

/// `run_script` with an explicit time budget. The deadline reaches everything the script can block on: the
/// JS interrupt handler, `sleep`, each command's own timeout, the socket read of a native call, and the
/// retries and respawns of the connection layer, none of which happen past it.
pub fn run_script_within(
    script: &str,
    flags: &Flags,
    daemon_opts: &DaemonOptions,
    timeout: u64,
) -> RunOutcome {
    let deadline = Instant::now() + Duration::from_millis(timeout);
    let _deadline_guard = crate::connection::DeadlineGuard::set(deadline);

    let fail = |text: String| RunOutcome { text, ok: false };
    let rt = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return fail(format!("failed to start JS runtime: {e}")),
    };
    rt.set_memory_limit(256 * 1024 * 1024);
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let ctx = match Context::full(&rt) {
        Ok(c) => c,
        Err(e) => return fail(format!("failed to start JS context: {e}")),
    };

    let cli = Cli { flags, daemon_opts };
    let (lent, _loan) = Lent::lend(&cli);
    let session = Rc::new(RefCell::new(Session {
        lent,
        relaunch_applied: false,
        deadline,
        timeout_ms: timeout,
    }));
    let mut logs: Vec<String> = Vec::new();

    let outcome: Result<Value, String> = ctx.with(|ctx| {
        let s2 = session.clone();
        let cmd_fn = Function::new(ctx.clone(), move |argv_json: String| -> String {
            let argv: Vec<String> = serde_json::from_str(&argv_json).unwrap_or_default();
            exec_one(&mut s2.borrow_mut(), argv).to_string()
        })
        .map_err(|e| e.to_string())?;
        let split_fn = Function::new(ctx.clone(), |line: String| -> Vec<String> {
            commands::shell_words_split(&line)
        })
        .map_err(|e| e.to_string())?;
        let sleep_fn = Function::new(ctx.clone(), move |ms: f64| {
            std::thread::sleep(clamp_sleep(
                ms,
                deadline.saturating_duration_since(Instant::now()),
            ));
        })
        .map_err(|e| e.to_string())?;
        let remaining_fn = Function::new(ctx.clone(), move || -> f64 {
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as f64
        })
        .map_err(|e| e.to_string())?;
        let g = ctx.globals();
        g.set("__cmd", cmd_fn).map_err(|e| e.to_string())?;
        g.set("__split", split_fn).map_err(|e| e.to_string())?;
        g.set("__sleep", sleep_fn).map_err(|e| e.to_string())?;
        g.set("__remaining", remaining_fn)
            .map_err(|e| e.to_string())?;
        g.set("__logCap", LOG_CAP_CHARS as f64)
            .map_err(|e| e.to_string())?;
        // The page-side library the resolver and the cleanup embed in their `eval` code.
        g.set("__DOM_LIB", crate::native::dom_js::DOM_LIB_JS)
            .map_err(|e| e.to_string())?;

        ctx.eval::<(), _>(PRELUDE)
            .catch(&ctx)
            .map_err(|e| format!("internal prelude error: {e}"))?;

        // The script body starts on line 1 so error line numbers match the file.
        let wrapped = format!(
            "(async () => {{ {script}\n}})().then(\
             v => JSON.stringify({{ok: true, value: v === undefined ? null : v}}), \
             e => JSON.stringify({{ok: false, error: String((e && e.message) || e), \
             stack: String((e && e.stack) || ''), step: e && e.abStep}}))"
        );
        let promise: Promise = ctx.eval(wrapped).catch(&ctx).map_err(|e| {
            if Instant::now() > deadline {
                format!("script timed out after {timeout} ms")
            } else {
                format!("script syntax error: {e}")
            }
        })?;
        let done: Result<String, String> = promise.finish::<String>().catch(&ctx).map_err(|e| {
            if Instant::now() > deadline {
                format!("script timed out after {timeout} ms")
            } else {
                format!("script error: {e}")
            }
        });
        // Whatever the outcome, let the page API clean up after itself.
        let _ = ctx.eval::<(), _>("T.finish()");
        let done = done?;
        if let Ok(arr) = ctx.globals().get::<_, Vec<String>>("__logs") {
            logs = arr;
        }
        // A script that used up its budget (a cut sleep, say) has timed out even if no instruction noticed.
        if Instant::now() > deadline {
            return Err(format!("script timed out after {timeout} ms"));
        }
        serde_json::from_str::<Value>(&done).map_err(|e| e.to_string())
    });

    let (mut text, ok) = match outcome {
        Ok(v) if v["ok"] == json!(true) => (render_value(&v["value"]), true),
        Ok(v) => {
            let mut msg = v["error"].as_str().unwrap_or("script failed").to_string();
            if std::env::var("AGENT_BROWSER_RUN_DEBUG").is_ok() {
                msg.push_str(&format!("\n{}", v["stack"].as_str().unwrap_or("")));
            }
            if let Some(line) = line_from_stack(v["stack"].as_str().unwrap_or("")) {
                msg.push_str(&format!(" (script line {line})"));
            }
            (msg, false)
        }
        Err(e) => (e, false),
    };
    if !logs.is_empty() {
        let joined = logs.join("\n");
        text = if text.is_empty() {
            joined
        } else {
            format!("{joined}\n{text}")
        };
    }
    if text.is_empty() && ok {
        // Not "ok": a model reads that as the value it asked for (a username, a title...).
        text = "(no value returned: the script returned undefined or null)".to_string();
    }
    RunOutcome { text, ok }
}

/// Line of the first `<anonymous>` frame in a QuickJS stack: the async
/// wrapper around the user script (the prelude's own frames are named).
fn line_from_stack(stack: &str) -> Option<u32> {
    let frame = stack
        .lines()
        .find(|l| l.trim_start().starts_with("at <anonymous>"))?;
    let inner = frame.rsplit('(').next()?.trim_end_matches(')');
    let mut parts = inner.rsplit(':');
    let _col = parts.next()?;
    parts.next()?.parse().ok()
}

/// `agent-browser run <file|->`: reads the script, runs it and prints the outcome like any other
/// command: page text goes through the same cap and content boundaries, `--json` prints the usual
/// `{success, data, error}`. Returns the exit status.
pub fn run_file(flags: &Flags, daemon_opts: &DaemonOptions, file: &str) -> i32 {
    let mut script = String::new();
    let read = if file == "-" {
        std::io::stdin().read_to_string(&mut script).map(|_| ())
    } else {
        std::fs::read_to_string(file).map(|s| script = s)
    };
    if let Err(e) = read {
        let message = format!("cannot read script {file}: {e}");
        if flags.json {
            print_response(false, None, Some(message));
        } else {
            eprintln!("{} {message}", crate::color::error_indicator());
        }
        return 1;
    }
    let RunOutcome { text, ok } = run_script(&script, flags, daemon_opts);
    let opts = OutputOptions::from_flags(flags);
    if !ok {
        if flags.json {
            print_response(false, None, Some(text));
        } else {
            eprintln!("{} {}", crate::color::error_indicator(), text);
        }
        return 1;
    }
    let shown = output::finalize_page_text(&text, None, &opts);
    if flags.json {
        print_response(true, Some(json!({ "output": shown })), None);
    } else {
        println!("{shown}");
    }
    0
}

/// Prints the usual response envelope.
fn print_response(success: bool, data: Option<Value>, error: Option<String>) {
    let resp = Response {
        success,
        data,
        error,
        ..Default::default()
    };
    println!("{}", serde_json::to_string(&resp).unwrap_or_default());
}

#[cfg(test)]
mod tests;

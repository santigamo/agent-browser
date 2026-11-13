//! Unit tests for code mode: the JS half is run against a mocked `__cmd`, so what each page.*
//! call sends to the CLI and how it reacts to the answers is checked without a browser.

use super::*;

/// Runs `script` against the prelude with `__cmd` answered by `respond` (argv, how many times
/// the same argv was seen before -> response JSON). Returns the argv of every command sent and
/// either `ok:<json value>` or `err:<message>`.
fn with_mock(script: &str, respond: fn(&[String], usize) -> Value) -> (Vec<Vec<String>>, String) {
    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let rt = Runtime::new().unwrap();
    let ctx = Context::full(&rt).unwrap();
    let out = ctx.with(|ctx| {
        let c2 = calls.clone();
        let cmd = Function::new(ctx.clone(), move |argv_json: String| -> String {
            let argv: Vec<String> = serde_json::from_str(&argv_json).unwrap();
            let n = c2.borrow().iter().filter(|a| **a == argv).count();
            c2.borrow_mut().push(argv.clone());
            respond(&argv, n).to_string()
        })
        .unwrap();
        let g = ctx.globals();
        g.set("__cmd", cmd).unwrap();
        g.set(
            "__split",
            Function::new(ctx.clone(), |l: String| commands::shell_words_split(&l)).unwrap(),
        )
        .unwrap();
        g.set(
            "__sleep",
            Function::new(ctx.clone(), |_ms: f64| {}).unwrap(),
        )
        .unwrap();
        g.set(
            "__remaining",
            Function::new(ctx.clone(), || 60_000f64).unwrap(),
        )
        .unwrap();
        g.set("__DOM_LIB", "(/* page library */ null)").unwrap();
        ctx.eval::<(), _>(PRELUDE).unwrap();
        let wrapped = format!(
            "(() => {{ try {{ return 'ok:' + JSON.stringify((() => {{ {script} }})()); }} \
             catch (e) {{ return 'err:' + e.message; }} }})()"
        );
        ctx.eval::<String, _>(wrapped).unwrap()
    });
    let calls = calls.borrow().clone();
    (calls, out)
}

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn ok() -> Value {
    json!({"success": true, "data": {}})
}

#[test]
fn goto_that_only_timed_out_on_load_continues_with_a_note() {
    let (calls, out) = with_mock(
        "page.goto('https://slow.test/'); return __logs;",
        |a, _| match a[0].as_str() {
            "open" => {
                json!({"success": true, "data": {"url": "https://slow.test/", "title": "Slow",
                "loadWarning": "the load event did not fire within 3000ms of DOMContentLoaded"}})
            }
            "get" => json!({"success": true, "data": {"url": "https://slow.test/"}}),
            _ => ok(),
        },
    );
    assert_eq!(calls[0], argv(&["open", "https://slow.test/"]));
    assert!(
        out.starts_with("ok:")
            && out.contains("[run note] page.goto(\\\"https://slow.test/\\\")")
            && out.contains("did not fire"),
        "{out}"
    );
    // a clean navigation adds nothing
    let (_, clean) = with_mock(
        "page.goto('https://fast.test/'); return __logs;",
        |a, _| match a[0].as_str() {
            "open" => {
                json!({"success": true, "data": {"url": "https://fast.test/", "title": "Fast"}})
            }
            _ => json!({"success": true, "data": {"url": "https://fast.test/"}}),
        },
    );
    assert_eq!(clean, "ok:[]");
}

#[test]
fn wait_for_asks_for_an_early_give_up_unless_a_timeout_is_given() {
    let (calls, _) = with_mock(
        "page.waitFor('#a'); page.waitFor('#b', 7000); page.waitFor({selector: '#c', timeout: 4000}); \
         page.waitFor({text: 'x'}); page.waitFor(500);",
        |_, _| ok(),
    );
    assert_eq!(calls[0], argv(&["wait", "#a", "--soft-timeout", "10000"]));
    assert_eq!(calls[1], argv(&["wait", "#b", "--timeout", "7000"]));
    assert_eq!(calls[2], argv(&["wait", "#c", "--timeout", "4000"]));
    assert_eq!(
        calls[3],
        argv(&["wait", "--text", "x", "--soft-timeout", "10000"])
    );
    assert_eq!(calls[4], argv(&["wait", "500"]));
}

#[test]
fn a_note_in_a_step_result_is_logged_once_with_the_step() {
    let (_, out) = with_mock("page.click('@e3'); return __logs;", |a, _| {
        match a[0].as_str() {
            "click" => json!({"success": true, "data": {"clicked": "@e3",
            "note": "@e3 is a hidden input; clicked its visible label instead"}}),
            _ => ok(),
        }
    });
    assert!(
        out.contains("[run note] page.click(\\\"@e3\\\"): @e3 is a hidden input"),
        "{out}"
    );
}

#[test]
fn css_target_not_rendered_yet_is_waited_for_then_retried() {
    let (calls, out) = with_mock("return page.text('h1');", |a, n| match a[0].as_str() {
        "get" if n == 0 => json!({"success": false, "error": "Element not found: h1."}),
        "get" => json!({"success": true, "data": {"text": "Hello"}}),
        _ => ok(),
    });
    assert_eq!(out, "ok:\"Hello\"");
    assert_eq!(
        calls,
        vec![
            argv(&["get", "text", "h1"]),
            argv(&["wait", "h1", "--state", "attached", "--timeout", "5000"]),
            argv(&["get", "text", "h1"]),
        ]
    );
}

#[test]
fn missing_ref_fails_at_once_and_names_the_step() {
    let (calls, out) = with_mock(
        "page.goto('https://a.test/'); page.click('@e9'); return 1;",
        |a, _| match a[0].as_str() {
            "click" => json!({"success": false, "error": "Element not found: @e9"}),
            "get" => json!({"success": true, "data": {"url": "https://a.test/"}}),
            _ => ok(),
        },
    );
    // goto's own URL read is not a step: the click is step 2.
    assert!(
        out.starts_with("err:step 2: page.click(\"@e9\") failed: Element not found: @e9"),
        "{out}"
    );
    assert!(out.contains("page: https://a.test/"), "{out}");
    assert!(!calls.iter().any(|c| c[0] == "wait"));
}

#[test]
fn css_target_that_never_appears_reports_the_wait() {
    let (_, out) = with_mock("page.fill('#q', 'x');", |a, _| match a[0].as_str() {
        "fill" => json!({"success": false, "error": "Element not found: #q"}),
        "wait" => json!({"success": false, "error": "Wait timed out"}),
        _ => ok(),
    });
    assert!(
        out.starts_with(
            "err:step 1: page.fill(\"#q\", \"x\") failed: Element not found: #q (waited 5s for it)"
        ),
        "{out}"
    );
}

#[test]
fn click_and_press_settle_but_reads_do_not() {
    assert!(settles_after(Some("click")));
    assert!(settles_after(Some("press")));
    assert!(!settles_after(Some("fill")));
    assert!(!settles_after(Some("snapshot")));
    assert!(!settles_after(None));
}

#[test]
fn other_errors_are_not_retried() {
    let (calls, _) = with_mock("page.click('#b');", |a, _| match a[0].as_str() {
        "click" => json!({"success": false, "error": "covered by <div#overlay>"}),
        _ => ok(),
    });
    assert_eq!(calls[0], argv(&["click", "#b"]));
    assert!(
        !calls.iter().any(|c| c[0] == "wait"),
        "no retry wait for non-missing errors"
    );
}

#[test]
fn failed_step_appends_url_title_and_the_closest_candidates() {
    let (_, out) = with_mock("page.click('#nope');", |a, _| match a[0].as_str() {
        "click" => json!({"success": false, "error": "Element not found: #nope"}),
        "wait" => json!({"success": false, "error": "Wait timed out"}),
        "get" if a[1] == "url" => {
            json!({"success": true, "data": {"url": "https://shop.test/cart"}})
        }
        "get" => json!({"success": true, "data": {"title": "Cart"}}),
        "snapshot" => json!({"success": true, "data": {"snapshot":
            "- link \"Home\" [ref=e1]\n- button \"Nope not really\" [ref=e2]\n- link \"Checkout\" [ref=e3]\n"}}),
        _ => ok(),
    });
    assert!(
        out.contains("page: https://shop.test/cart | \"Cart\""),
        "{out}"
    );
    // the closest candidate (shares the word "nope") is listed first
    assert!(
        out.contains("nearest: @e2 button \"Nope not really\"; @e1 link \"Home\""),
        "{out}"
    );
    assert!(out.len() < 1800, "context stays compact: {}", out.len());
}

#[test]
fn failed_input_target_lists_the_controls_that_may_create_it() {
    let (_, out) = with_mock("page.fill('input[type=search]', 'x');", |a, _| {
        match a[0].as_str() {
            "fill" => json!({"success": false, "error": "Element not found: input[type=search]"}),
            "wait" => json!({"success": false, "error": "Wait timed out"}),
            "get" if a[1] == "url" => {
                json!({"success": true, "data": {"url": "https://docs.test/"}})
            }
            "get" => json!({"success": true, "data": {"title": "Docs"}}),
            "snapshot" => json!({"success": true, "data": {"snapshot":
            "- link \"Home\" [ref=e1]\n- button \"Search\" [ref=e2]\n- link \"Plans\" [ref=e3]\n"}}),
            _ => ok(),
        }
    });
    assert!(
        out.contains("a control may create it on demand: click @e2 button \"Search\", then retry"),
        "{out}"
    );
}

#[test]
fn a_gated_action_that_needs_confirmation_fails_the_step_and_names_the_confirm_id() {
    // The daemon answers a parked action with success and `confirmation_required`; nothing ran.
    let parked = Response {
        success: true,
        data: Some(
            json!({"confirmation_required": true, "confirmation_id": "c7", "action": "click"}),
        ),
        ..Default::default()
    };
    let v = response_json(&parked);
    assert_eq!(v["success"], false);
    assert_eq!(v["code"], "confirmation_required");
    let msg = v["error"].as_str().unwrap();
    assert!(
        msg.contains("c7") && msg.contains("NOT run") && msg.contains("confirm c7"),
        "{msg}"
    );
    // and the script sees a failed step
    let (calls, out) = with_mock("page.click('#buy'); return 'done';", |a, _| {
        match a[0].as_str() {
            "click" => json!({"success": false, "code": "confirmation_required",
            "error": "click needs confirmation and was NOT run (confirm id c7): approve it"}),
            _ => ok(),
        }
    });
    assert!(
        out.starts_with("err:step 1: page.click(\"#buy\") failed: click needs confirmation"),
        "{out}"
    );
    assert!(!calls.iter().any(|c| c[0] == "wait"));
    // an ordinary response is passed through
    let fine = Response {
        success: true,
        data: Some(json!({"clicked": "#x"})),
        ..Default::default()
    };
    assert_eq!(response_json(&fine)["data"]["clicked"], "#x");
}

#[test]
fn sleep_and_operation_timeouts_are_cut_to_the_time_left() {
    let left = Duration::from_millis(300);
    assert_eq!(clamp_sleep(10_000.0, left), Duration::from_millis(301));
    assert_eq!(clamp_sleep(100.0, left), Duration::from_millis(100));
    assert_eq!(clamp_sleep(-5.0, left), Duration::ZERO);
    let mut wait = json!({"action": "wait", "timeout": 25_000});
    clamp_op_timeout(&mut wait, left);
    assert_eq!(wait["timeout"], 300);
    let mut shorter = json!({"action": "wait", "timeout": 100});
    clamp_op_timeout(&mut shorter, left);
    assert_eq!(shorter["timeout"], 100);
    let mut none = json!({"action": "click"});
    clamp_op_timeout(&mut none, left);
    assert!(none.get("timeout").is_none());
}

#[test]
fn a_script_that_sleeps_or_spins_past_its_budget_ends_at_the_deadline() {
    let flags = crate::commands::tests::default_flags();
    let opts = DaemonOptions::bare();
    for script in [
        "sleep(20000); return 1;",
        "while (true) {}",
        "sleep(400); page.goto('http://never.test/'); return 2;",
    ] {
        let t = Instant::now();
        let out = run_script_within(script, &flags, &opts, 300);
        assert!(!out.ok, "{script}: {}", out.text);
        assert!(
            out.text.contains("script timed out after 300 ms"),
            "{script}: {}",
            out.text
        );
        assert!(
            t.elapsed() < Duration::from_secs(3),
            "{script} took {:?}",
            t.elapsed()
        );
    }
}

#[test]
fn the_borrowed_cli_state_is_unreachable_once_the_run_has_ended() {
    let flags = crate::commands::tests::default_flags();
    let opts = DaemonOptions::bare();
    let lent = {
        let cli = Cli {
            flags: &flags,
            daemon_opts: &opts,
        };
        let (lent, loan) = Lent::lend(&cli);
        assert_eq!(
            lent.with(|c| c.flags.session.clone()),
            Some("test".to_string())
        );
        drop(loan);
        lent
    };
    // a native function that outlives the run finds nothing to touch
    assert_eq!(lent.with(|c| c.flags.session.clone()), None);
    let mut s = Session {
        lent,
        relaunch_applied: false,
        deadline: Instant::now() + Duration::from_secs(60),
        timeout_ms: 60_000,
    };
    let v = exec_one(&mut s, argv(&["get", "url"]));
    assert_eq!(v["success"], false);
    assert!(v["error"].as_str().unwrap().contains("already ended"));
}

#[test]
fn console_log_is_capped_while_collecting() {
    let (_, out) = with_mock(
        "for (let i = 0; i < 50000; i++) console.log('line ' + i + ' padding padding padding'); \
         console.log('s'); return [__logs.length, __logs.join('\\n').length, __logs[__logs.length - 1]];",
        |_, _| ok(),
    );
    let v: Value = serde_json::from_str(out.strip_prefix("ok:").unwrap()).unwrap();
    assert!(v[1].as_u64().unwrap() <= 12_200, "{out}");
    assert!(v[0].as_u64().unwrap() < 600, "{out}");
    assert!(
        v[2].as_str().unwrap().contains("further log lines dropped"),
        "{out}"
    );
}

#[test]
fn stack_line_is_extracted() {
    assert_eq!(
        line_from_stack("    at <anonymous> (eval_script:7:12)\n"),
        Some(7)
    );
}

#[test]
fn selector_strings_split_on_the_chain_operator_only_outside_quotes() {
    let (_, out) = with_mock(
        r#"return [
            parseSel('a >> text=Next').map((s) => s.k),
            parseSel('p:has-text("x >> y")').map((s) => s.k + ':' + (s.v || '')),
            parseSel("p:has-text('a >> b') >> text=c").map((s) => s.k),
            parseSel('div[title="a >> b"]').map((s) => s.k + ':' + s.v),
            parseSel(':has-text("z")').map((s) => s.k + (s.bare ? ':bare' : '')),
            parseSel('li:has-text("z") .t').map((s) => s.k + (s.bare ? ':bare' : '')),
            ENGINE.test('#plain .css'), ENGINE.test('a >> b'), ENGINE.test('text=x'),
        ];"#,
        |_, _| ok(),
    );
    let v: Value = serde_json::from_str(out.strip_prefix("ok:").unwrap()).unwrap();
    assert_eq!(v[0], json!(["css", "text"]));
    assert_eq!(v[1], json!(["css:p", "has-text:x >> y"]));
    assert_eq!(v[2], json!(["css", "has-text", "text"]));
    assert_eq!(v[3], json!(["css:div[title=\"a >> b\"]"]));
    // a filter with nothing before it applies to every element and is marked bare
    assert_eq!(v[4], json!(["css", "has-text:bare"]));
    assert_eq!(v[5], json!(["css", "has-text", "css"]));
    assert_eq!(v[6], false);
    assert_eq!(v[7], true);
    assert_eq!(v[8], true);
}

#[test]
fn plain_css_and_refs_never_go_through_the_resolver() {
    let (calls, _) = with_mock(
        "page.click('#a'); page.fill('@e3', 'x'); page.count('li.x');",
        |_, _| json!({"success": true, "data": {"count": 2}}),
    );
    assert!(!calls.iter().any(|c| c[0] == "eval"));
}

#[test]
fn locator_steps_send_the_resolver_with_the_spec_and_act_on_the_marker() {
    let (calls, out) = with_mock(
        "page.getByRole('button', {name: /go/i}).click(); \
         return page.locator('a:has-text(\"Next\") >> nth=1').count();",
        |a, _| match a[0].as_str() {
            "eval" => {
                json!({"success": true, "data": {"result": {"n": 3, "all": 3, "visible": 1, "picked": true, "items": []}}})
            }
            _ => ok(),
        },
    );
    assert_eq!(out, "ok:3");
    assert_eq!(calls[0][0], "eval");
    assert!(
        calls[0][1].contains("\"k\":\"role\"") && calls[0][1].contains("\"re\":\"go\""),
        "{}",
        calls[0][1]
    );
    assert_eq!(calls[1][0], "click");
    assert!(
        calls[1][1].starts_with("[data-ab-loc=\"l") && calls[1][1].ends_with("\"]"),
        "{:?}",
        calls[1]
    );
    // a count resolves without tagging
    assert!(calls[2][1].contains("\"tag\":false"), "{}", calls[2][1]);
}

#[test]
fn an_eval_that_fails_is_reported_as_such_and_never_as_not_found() {
    let (_, out) = with_mock("page.getByText('x').click();", |a, _| match a[0].as_str() {
        "eval" => json!({"success": false, "error": "Execution context was destroyed"}),
        _ => ok(),
    });
    assert!(
        out.contains("could not evaluate the locator text=x: Execution context was destroyed")
            && !out.contains("Element not found"),
        "{out}"
    );
}

#[test]
fn unsupported_keys_are_type_errors_that_name_them() {
    let (_, out) = with_mock("page.click('#a', {force: true});", |_, _| ok());
    assert!(
        out.starts_with("err:page.click(): unsupported option 'force'"),
        "{out}"
    );
    let (_, out) = with_mock(
        "page.getByText('x').filter({has: page.locator('a')});",
        |_, _| ok(),
    );
    assert!(
        out.contains("unsupported option 'has' (supported: hasText)"),
        "{out}"
    );
    let (_, out) = with_mock("page.waitFor({selector: '#a', state: 'ready'});", |_, _| {
        ok()
    });
    assert!(out.contains("unsupported state 'ready'"), "{out}");
}

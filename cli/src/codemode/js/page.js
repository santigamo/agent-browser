// Added in the browser-duel fork of vercel-labs/agent-browser v0.43.0 (Apache-2.0); see CHANGES-FORK.md.
// Code mode: the `page` API.
//
// `__cmd(argvJson)` runs one normal agent-browser command (same parser, same daemon, same policy)
// and returns the response as JSON. Each page.* call is one step; a failing step stops the script.
// Options a step does not support are an error, never silently ignored: a script that asks for
// `force: true` must not run as if it had not.
const page = (() => {
  let step = 0;

  // A CSS target that is not in the DOM yet (page still loading or rendering) gets one bounded
  // wait for it to attach, then the step is retried once. Refs are never waited for: a missing
  // ref will not come back.
  const AUTO_WAIT_MS = 5000;
  const waitable = (t) => typeof t === "string" && t !== "" && !/^(@|ref=)/.test(t) && !T.isMarker(t);
  const missing = (r) => !r.success && /not found|no element/i.test(r.error || "");

  // `target` is the step's selector when it has one; `step` counts page.* calls.
  const run = (name, argv, shown, target, waitMs = AUTO_WAIT_MS) => {
    step++;
    let r = exec(argv);
    if (missing(r) && T.isMarker(target)) {
      // The marker left the page between resolving and acting (it re-rendered): resolve once more.
      if (T.reresolve(target)) r = exec(argv);
    } else if (missing(r) && waitable(target)) {
      const w = exec(["wait", target, "--state", "attached", "--timeout", waitMs]);
      if (w.success) r = exec(argv);
      else r.error = `${r.error} (waited ${waitMs / 1000}s for it)`;
    }
    if (!r.success) throw failure(name, shown, T.rename(r.error || "unknown error"), target);
    if (r.data && r.data.note) __log(`[run note] page.${name}(${shown ?? ""}): ${r.data.note}`);
    return r.data ?? {};
  };
  const pick = (d, ...keys) => {
    for (const k of keys) if (d[k] !== undefined && d[k] !== null) return d[k];
    return d;
  };
  // Current URL after a navigation step, without counting as a step of its own.
  const here = () => {
    const r = exec(["get", "url"]);
    return r.success ? pick(r.data ?? {}, "url") : "";
  };

  // Compact page context for a failed step: url, title, the interactive elements closest to the
  // target and, when the target looks like an input that is not there, the controls that may
  // create it on demand.
  const GENERIC = new RegExp(
    "^(input|type|name|role|text|css|label|placeholder|has|nth|first|last|exact|div|span|the" +
      "|and|data|testid|id|class)$"
  );
  const INPUT_LIKE = new RegExp(
    "input|textbox|combobox|searchbox|textarea|field|type=|placeholder" +
      "|role=(textbox|combobox|searchbox)|label=",
    "i"
  );
  const SEARCH_NAME = /name=["']?(q|s|query|keywords?|term|search)\b|type=["']?search|role=searchbox/i;
  const ITEM = /^\s*-\s+(\S+)(?:\s+"(.*?)")?.*\[ref=(e\d+)/;
  const context = (target) => {
    try {
      if (__remaining() <= 0) return "";
      const u = exec(["get", "url"]);
      const ti = exec(["get", "title"]);
      const snap = exec(["snapshot", "-i"]);
      const text = snap.success ? (snap.data?.snapshot ?? "") : "";
      const items = [];
      for (const line of String(text).split("\n")) {
        const m = ITEM.exec(line);
        if (m) items.push({ role: m[1], name: m[2] || "", ref: m[3] });
      }
      const t = String(target || "");
      const words = t
        .toLowerCase()
        .split(/[^a-z0-9]+/)
        .filter((w) => w.length >= 2 && !GENERIC.test(w));
      // search inputs are named q, s, query, keywords...: the control that opens them says "search"
      if (SEARCH_NAME.test(t) && !words.includes("search")) words.push("search");
      const score = (it) =>
        words.reduce((n, w) => n + ((it.name.toLowerCase() + " " + it.role).includes(w) ? 1 : 0), 0);
      const ranked = items
        .map((it, i) => ({ it, i, s: score(it) }))
        .sort((a, b) => b.s - a.s || a.i - b.i)
        .slice(0, 12);
      const fmt = (it) => `@${it.ref} ${it.role}${it.name ? " " + JSON.stringify(it.name.slice(0, 40)) : ""}`;
      let out = `\n  page: ${String(u.data?.url ?? "").slice(0, 120)} | ${JSON.stringify(ti.data?.title ?? "")}`;
      const inputShown = items.some((it) => /^(textbox|combobox|searchbox)$/.test(it.role) && score(it) > 0);
      if (INPUT_LIKE.test(t) && !inputShown) {
        const trigger = items.filter((it) => /^(button|link|menuitem|tab)$/.test(it.role) && score(it) > 0);
        if (trigger.length) {
          out += "\n  not on the page yet? a control may create it on demand: click ";
          out += trigger.slice(0, 3).map(fmt).join(" or ") + ", then retry";
        }
      }
      if (ranked.length) out += `\n  nearest: ${ranked.map((r) => fmt(r.it)).join("; ")}`;
      return out.length > 1500 ? out.slice(0, 1497) + "..." : out;
    } catch (e) {
      return "";
    }
  };
  const RECOVERABLE = /not found|no element|timed out|timeout|matches \d+|ambiguous|strict mode|not visible|covered/i;
  const failure = (name, shown, message, target) => {
    message += T.ambiguity(target);
    const what = `step ${step}: page.${name}(${shown ?? ""}) failed: ${message}`;
    const e = new Error(what + (RECOVERABLE.test(message) ? context(target) : ""));
    e.abStep = step;
    return e;
  };

  // Options: an object whose keys are all in `allowed`, else a TypeError that names the key.
  const only = (o, allowed, where) => {
    if (o === undefined || o === null) return {};
    if (typeof o !== "object" || Array.isArray(o) || o instanceof RegExp) {
      throw new TypeError(`${where}: options must be an object`);
    }
    for (const k of Object.keys(o)) {
      if (!allowed.includes(k)) {
        const ok = allowed.length ? `supported: ${allowed.join(", ")}` : "this call takes no options";
        throw new TypeError(`${where}: unsupported option '${k}' (${ok})`);
      }
    }
    return o;
  };
  // How long a step waits for its target to exist: the caller's `timeout`, else 5 s.
  const budget = (o) => (typeof o.timeout === "number" && o.timeout >= 0 ? o.timeout : AUTO_WAIT_MS);
  const isOpts = (x) => !!x && typeof x === "object" && !Array.isArray(x) && !(x instanceof RegExp);
  const fails = (name, t) => (msg) => {
    step++;
    throw failure(name, q(T.describe(t)), msg, T.describe(t));
  };
  const resolveFor = (t, name, mode, o) => T.resolve(t, name, mode, fails(name, t), budget(o ?? {}));

  // One step on a target: resolve it, run the command, return the data.
  // `build(selector, ...args)` makes the argv; `shown(description, ...args)` the text for messages.
  const act = (name, build, shown) => (t, args, opts) => {
    const o = only(opts, ["timeout"], `page.${name}()`);
    const sel = resolveFor(t, name, undefined, o);
    const text = shown ? shown(T.describe(t), ...args) : q(T.describe(t));
    return run(name, build(sel, ...args), text, sel, budget(o));
  };
  const simple = (name, ...cmd) => {
    const call = act(name, (s) => [...cmd, s]);
    return (t, o) => (call(t, [], o), undefined);
  };
  const read = (name, cmd, ...keys) => {
    const call = act(name, (s) => [...cmd, s]);
    return (t, o) => pick(call(t, [], o), ...keys);
  };
  // A typed value (a password, a token) is not echoed into messages: the Rust side decides which
  // commands, the same rule as the batch headers.
  const typed = (name, v) => (__redacts(name) ? '"<text>"' : q(v));
  const withValue = (name, cmd) => {
    const call = act(name, (s, v) => [...cmd, s, v], (d, v) => `${q(d)}, ${typed(name, v)}`);
    return (t, v, o) => (call(t, [v], o), undefined);
  };

  const WAIT_UNTIL = { load: "load", domcontentloaded: "domcontentloaded", networkidle: "networkidle", commit: "none" };
  const STATES = ["visible", "hidden", "attached", "detached"];

  const p = {
    goto: (url, opts) => {
      const o = only(opts, ["waitUntil"], "page.goto()");
      const a = ["open", url];
      if (o.waitUntil !== undefined) {
        if (!(o.waitUntil in WAIT_UNTIL)) {
          const ok = Object.keys(WAIT_UNTIL).join(", ");
          throw new TypeError(`page.goto(): unsupported waitUntil '${o.waitUntil}' (supported: ${ok})`);
        }
        a.push("--wait-until", WAIT_UNTIL[o.waitUntil]);
      }
      const d = run("goto", a, q(url));
      if (d && d.loadWarning) __log(`[run note] page.goto(${q(url)}): ${d.loadWarning}`);
      return here();
    },
    open: (url, o) => p.goto(url, o),
    back: () => (run("back", ["back"]), here()),
    reload: () => (run("reload", ["reload"]), here()),
    snapshot: (opts = {}) => {
      const keys = ["interactive", "compact", "urls", "all", "depth", "selector", "find", "hidden"];
      const o = only(opts, keys, "page.snapshot()");
      const a = ["snapshot"];
      if (o.interactive !== false && o.interactive !== undefined) a.push("-i");
      if (o.compact) a.push("-c");
      if (o.urls) a.push("-u");
      if (o.all) a.push("--all");
      if (o.depth) a.push("-d", o.depth);
      if (o.selector) a.push("-s", o.selector);
      // find: matched lines with their ancestors, saying whether nothing matched or the list was
      // cut. hidden: also the text a snapshot omits (closed <details>, hidden, display:none),
      // labelled by container.
      if (o.find instanceof RegExp) {
        throw new TypeError("page.snapshot(): find takes a string; a regular expression is not supported");
      }
      if (o.find !== undefined && o.find !== "") a.push("--find", String(o.find));
      if (o.hidden) a.push("--hidden-text");
      const d = run("snapshot", a, JSON.stringify(o));
      return typeof d === "string" ? d : (d.snapshot ?? JSON.stringify(d));
    },
    click: simple("click", "click"),
    dblclick: simple("dblclick", "dblclick"),
    hover: simple("hover", "hover"),
    focus: simple("focus", "focus"),
    check: simple("check", "check"),
    uncheck: simple("uncheck", "uncheck"),
    fill: withValue("fill", ["fill"]),
    type: withValue("type", ["type"]),
    // press(key) or Playwright's press(target, key): focus the target first
    press: (a, b, opts) => {
      if (b === undefined) return (run("press", ["press", a], q(a)), undefined);
      only(opts, [], "page.press()");
      p.focus(a);
      run("press", ["press", b], `${q(T.describe(a))}, ${q(b)}`);
    },
    keyboard: {
      press: (k) => (run("press", ["press", k], q(k)), undefined),
      type: (txt) => (run("type", ["keyboard", "type", txt], typed("keyboard", txt)), undefined),
    },
    select: (t, ...rest) => {
      const opts = rest.length && isOpts(rest[rest.length - 1]) ? rest.pop() : undefined;
      const vals = rest.flat();
      const call = act("select", (s) => ["select", s, ...vals], (d) => [d, ...vals].map(q).join(", "));
      call(t, [], opts);
    },
    scroll: (dir = "down", px) => {
      run("scroll", px ? ["scroll", dir, px] : ["scroll", dir], q(dir));
    },
    text: read("text", ["get", "text"], "text"),
    html: read("html", ["get", "html"], "html"),
    value: read("value", ["get", "value"], "value"),
    attr: (t, name, opts) => {
      const call = act("attr", (s, n) => ["get", "attr", s, n], (d, n) => `${q(d)}, ${q(n)}`);
      return pick(call(t, [name], opts), "value", "attr");
    },
    count: (t) => {
      const n = T.count(t, fails("count", t));
      if (n !== undefined) {
        step++;
        return n;
      }
      return pick(run("count", ["get", "count", t], q(t)), "count");
    },
    url: () => pick(run("url", ["get", "url"]), "url"),
    title: () => pick(run("title", ["get", "title"]), "title"),
    // False when nothing matches: asking is not an error. No options are supported.
    isVisible: (t, opts) => {
      only(opts, [], "page.isVisible()");
      const sel = T.resolve(t, "isVisible", "soft", fails("isVisible", t));
      if (sel === null) return (step++, false);
      step++;
      const r = exec(["is", "visible", sel]);
      if (missing(r)) return false;
      if (!r.success) throw failure("isVisible", q(T.describe(t)), T.rename(r.error || "unknown error"), sel);
      return pick(r.data ?? {}, "visible");
    },
    // waitFor("#sel") | waitFor(locator) | waitFor({text, selector, url, load, ms, fn, state, timeout})
    // A selector or text wait with no timeout from the caller gets a 10 s ceiling that it may
    // give up on early (settled page, nothing visible); a timeout the caller gives is waited out.
    // `state` is visible (default), hidden, attached or detached, for selectors and locators.
    waitFor: (w, timeout) => {
      const isSel = typeof w === "string" || T.isLoc(w);
      const o = isSel ? { selector: w } : typeof w === "number" ? { ms: w } : w;
      only(o, ["selector", "text", "url", "load", "ms", "fn", "state", "timeout"], "page.waitFor()");
      const state = o.state === undefined ? "visible" : o.state;
      if (!STATES.includes(state)) {
        const ok = STATES.join(", ");
        throw new TypeError(`page.waitFor(): unsupported state '${state}' (supported: ${ok})`);
      }
      if (o.state !== undefined && !o.selector) {
        throw new TypeError("page.waitFor(): state applies to a selector or locator");
      }
      const chosen = o.timeout || timeout;
      // How the call is shown in a failure: the locator by its description, not its internals.
      const shown = isSel
        ? q(T.describe(w))
        : JSON.stringify(o.selector && T.isLoc(o.selector) ? { ...o, selector: T.describe(o.selector) } : w);
      if (o.selector && T.needsResolver(o.selector)) {
        step++;
        const ms = chosen || 10000;
        if (!T.until(o.selector, state, ms, fails("waitFor", o.selector))) {
          const what = `Timed out after ${ms}ms waiting for ${T.describe(o.selector)} to be ${state}`;
          throw failure("waitFor", shown, what, T.describe(o.selector));
        }
        return;
      }
      const a = ["wait"];
      let target;
      if (o.selector) {
        target = o.selector;
        a.push(target);
        if (state !== "visible") a.push("--state", state);
      } else if (o.text) a.push("--text", o.text);
      else if (o.url) a.push("--url", o.url);
      else if (o.load) a.push("--load", o.load === true ? "load" : o.load);
      else if (o.ms) a.push(o.ms);
      else if (o.fn) a.push("--fn", o.fn);
      if (!o.ms) a.push((o.selector || o.text) && !chosen ? "--soft-timeout" : "--timeout", chosen || 10000);
      else if (chosen) a.push("--timeout", chosen);
      run("waitFor", a, shown, o.text || o.url || target);
    },
    // Playwright names models reach for
    waitForSelector: (sel, opts) => {
      const o = only(opts, ["state", "timeout"], "page.waitForSelector()");
      return p.waitFor({ selector: sel, state: o.state, timeout: o.timeout });
    },
    waitForURL: (u, opts) => {
      const o = only(opts, ["timeout"], "page.waitForURL()");
      return p.waitFor({ url: u, timeout: o.timeout });
    },
    waitForTimeout: (ms) => (__sleep(ms), undefined),
    innerText: (t, o) => p.text(t, o),
    textContent: (t, o) => p.text(t, o),
    inputValue: (t, o) => p.value(t, o),
    getAttribute: (t, n, o) => p.attr(t, n, o),
    evaluate: (f, ...args) => {
      const call = args.map((x) => JSON.stringify(x)).join(",");
      const code = typeof f === "function" ? `(${f.toString()})(${call})` : String(f);
      const d = run("evaluate", ["eval", code], q(code.length > 60 ? code.slice(0, 57) + "..." : code));
      return d && typeof d === "object" && "result" in d ? d.result : d;
    },
    // Escape hatch: any normal CLI command, e.g. page.cmd("find role button click --name Submit")
    cmd: (line) => {
      const argv = Array.isArray(line) ? line : __split(line);
      return run("cmd", argv, q(argv.join(" ")));
    },
    sleep: (ms) => (__sleep(ms), undefined),
  };
  p.__only = only;
  p.__steps = (t) => (T.isLoc(t) ? t.steps : parseSel(t));
  p.__describe = (t) => T.describe(t);
  return p;
})();
globalThis.sleep = (ms) => page.sleep(ms);

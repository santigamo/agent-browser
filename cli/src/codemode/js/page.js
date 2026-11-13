// Code mode, part 3 of 3: the `page` API.
//
// `__cmd(argvJson)` runs one normal agent-browser command (same parser, same daemon, same policy)
// and returns the response as JSON. Each page.* call is one step; a failing step stops the script.
const page = (() => {
  let step = 0;
  const q = (v) => (typeof v === "string" ? JSON.stringify(v) : String(v));
  const exec = (argv) => JSON.parse(__cmd(JSON.stringify(argv.map(String))));

  // A CSS target that is not in the DOM yet (page still loading or rendering) gets one bounded
  // wait for it to attach, then the step is retried once. Refs are never waited for: a missing
  // ref will not come back.
  const AUTO_WAIT_MS = 5000;
  const waitable = (t) => typeof t === "string" && t !== "" && !/^(@|ref=)/.test(t);
  const missing = (r) => !r.success && /not found|no element/i.test(r.error || "");

  // `target` is the step's selector when it has one; `step` counts page.* calls.
  const run = (name, argv, shown, target) => {
    step++;
    let r = exec(argv);
    if (missing(r) && waitable(target)) {
      const w = exec(["wait", target, "--state", "attached", "--timeout", AUTO_WAIT_MS]);
      if (w.success) r = exec(argv);
      else r.error = `${r.error} (waited ${AUTO_WAIT_MS / 1000}s for it)`;
    }
    if (!r.success) throw failure(name, shown, r.error || "unknown error", target);
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

  // One step on a target: resolve it, run the command, return the data.
  const act = (name, build, shown) => (t, ...args) => {
    const sel = T.resolve(t, name);
    const argv = build(sel, ...args);
    return run(name, argv, shown ? shown(T.describe(t), ...args) : q(T.describe(t)), sel);
  };
  const first = (name, build, ...keys) => {
    const call = act(name, build);
    return (t, ...args) => pick(call(t, ...args), ...keys);
  };

  const p = {
    goto: (url) => {
      const d = run("goto", ["open", url], q(url));
      if (d && d.loadWarning) __log(`[run note] page.goto(${q(url)}): ${d.loadWarning}`);
      return here();
    },
    open: (url) => p.goto(url),
    back: () => (run("back", ["back"]), here()),
    reload: () => (run("reload", ["reload"]), here()),
    snapshot: (o = {}) => {
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
      if (o.find !== undefined && o.find !== "") {
        a.push("--find", o.find instanceof RegExp ? o.find.source : String(o.find));
      }
      if (o.hidden) a.push("--hidden-text");
      const d = run("snapshot", a, JSON.stringify(o));
      return typeof d === "string" ? d : (d.snapshot ?? JSON.stringify(d));
    },
    click: (t) => (act("click", (s) => ["click", s])(t), undefined),
    dblclick: (t) => (act("dblclick", (s) => ["dblclick", s])(t), undefined),
    hover: (t) => (act("hover", (s) => ["hover", s])(t), undefined),
    focus: (t) => (act("focus", (s) => ["focus", s])(t), undefined),
    fill: (t, v) => {
      act("fill", (s, x) => ["fill", s, x], (d, x) => `${q(d)}, ${q(x)}`)(t, v);
    },
    type: (t, v) => {
      act("type", (s, x) => ["type", s, x], (d, x) => `${q(d)}, ${q(x)}`)(t, v);
    },
    // press(key) or Playwright's press(target, key): focus the target first
    press: (a, b) => {
      if (b === undefined) return (run("press", ["press", a], q(a)), undefined);
      act("focus", (s) => ["focus", s])(a);
      run("press", ["press", b], `${q(T.describe(a))}, ${q(b)}`);
    },
    keyboard: {
      press: (k) => (run("press", ["press", k], q(k)), undefined),
      type: (txt) => (run("type", ["keyboard", "type", txt], q(txt)), undefined),
    },
    check: (t) => (act("check", (s) => ["check", s])(t), undefined),
    uncheck: (t) => (act("uncheck", (s) => ["uncheck", s])(t), undefined),
    select: (t, ...vals) => {
      const flat = vals.flat();
      act("select", (s) => ["select", s, ...flat], (d) => [d, ...flat].map(q).join(", "))(t);
    },
    scroll: (dir = "down", px) => {
      run("scroll", px ? ["scroll", dir, px] : ["scroll", dir], q(dir));
    },
    text: first("text", (s) => ["get", "text", s], "text"),
    html: first("html", (s) => ["get", "html", s], "html"),
    value: first("value", (s) => ["get", "value", s], "value"),
    attr: (t, name) => {
      const call = act("attr", (s, n) => ["get", "attr", s, n], (d, n) => `${q(d)}, ${q(n)}`);
      return pick(call(t, name), "value", "attr");
    },
    count: (t) => {
      const n = T.count(t);
      if (n !== undefined) return n;
      return pick(run("count", ["get", "count", t], q(t)), "count");
    },
    url: () => pick(run("url", ["get", "url"]), "url"),
    title: () => pick(run("title", ["get", "title"]), "title"),
    isVisible: (t) => {
      const sel = T.resolve(t, "isVisible", true);
      return pick(run("isVisible", ["is", "visible", sel], q(T.describe(t))), "visible");
    },
    // waitFor("#sel") | waitFor({text, selector, url, load, ms, timeout, fn})
    // A selector or text wait with no timeout from the caller gets a 10 s ceiling that it may
    // give up on early (settled page, nothing visible); a timeout the caller gives is waited out.
    waitFor: (w, timeout) => {
      const o = typeof w === "string" ? { selector: w } : typeof w === "number" ? { ms: w } : w;
      const a = ["wait"];
      let target;
      if (o.selector) {
        target = T.resolve(o.selector, "waitFor");
        a.push(target);
      } else if (o.text) a.push("--text", o.text);
      else if (o.url) a.push("--url", o.url);
      else if (o.load) a.push("--load", o.load === true ? "load" : o.load);
      else if (o.ms) a.push(o.ms);
      else if (o.fn) a.push("--fn", o.fn);
      const chosen = o.timeout || timeout;
      if (!o.ms) a.push((o.selector || o.text) && !chosen ? "--soft-timeout" : "--timeout", chosen || 10000);
      else if (chosen) a.push("--timeout", chosen);
      const shown = typeof o.selector === "string" || !o.selector ? JSON.stringify(w) : q(T.describe(w));
      run("waitFor", a, shown, o.text || o.url || (typeof target === "string" ? target : undefined));
    },
    // Playwright names models reach for
    waitForSelector: (sel, o) => p.waitFor(sel, o && o.timeout),
    waitForURL: (u, o) => p.waitFor({ url: u }, o && o.timeout),
    waitForTimeout: (ms) => (__sleep(ms), undefined),
    innerText: (t) => p.text(t),
    textContent: (t) => p.text(t),
    inputValue: (t) => p.value(t),
    getAttribute: (t, n) => p.attr(t, n),
    evaluate: (f, ...args) => {
      const code =
        typeof f === "function" ? `(${f.toString()})(${args.map((x) => JSON.stringify(x)).join(",")})` : String(f);
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
  p.__T = T;
  return p;
})();
globalThis.sleep = (ms) => page.sleep(ms);

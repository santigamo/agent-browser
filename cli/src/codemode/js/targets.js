// Code mode: what a step's target may be, and how it is resolved.
//
// A target is a CSS selector or an @ref, passed to the command as it is; or a Playwright-style
// selector string or locator object (selector.js, locator.js), which the resolver (resolver.js,
// run in the page) turns into one element. That element gets a data-ab-loc marker, and the step
// then runs as an ordinary command on `[data-ab-loc="<id>"]`. Everything that touches targets goes
// through `T`, so the page API does not care how a target is resolved.
//
// Only an ACTIONABLE element is ever chosen (visible, or a form control a user can really reach);
// a locator whose matches are all hidden fails with a diagnostic instead of acting on one. A
// locator that matches several visible elements acts on the first and says so in the output.
// The marker value is unique to this script run, so two scripts driving the same page at once
// cannot move or remove each other's marker; the script's steps are synchronous (one QuickJS
// thread), so nothing else runs between a resolve and its act. Markers are removed at the end.

const q = (v) => (typeof v === "string" ? JSON.stringify(v) : String(v));
const exec = (argv) => JSON.parse(__cmd(JSON.stringify(argv.map(String))));

const T = (() => {
  const isLoc = (t) => !!t && typeof t === "object" && t.__loc === true;
  const needsResolver = (t) => isLoc(t) || (typeof t === "string" && ENGINE.test(t));
  const stepsOf = (t) => (isLoc(t) ? t.steps : parseSel(t));
  const describe = (t) => (isLoc(t) ? t.desc : String(t));

  const WAIT_MS = 5000;
  const LOC_ID = "l" + Math.random().toString(36).slice(2, 10);
  const LOC_SEL = `[data-ab-loc="${LOC_ID}"]`;
  const RESOLVER_SRC = resolver.toString();
  let last = null; // the locator the marker currently stands for
  let tagged = false;

  // One resolver call. A failure of the call itself is an error, never "no match": the caller
  // sees why the locator could not be evaluated instead of a misleading "not found".
  const resolveOnce = (spec) => {
    const code = `(${RESOLVER_SRC})(${JSON.stringify(spec)}, ${__DOM_LIB})`;
    const r = exec(["eval", code]);
    if (!r.success) return { n: 0, all: 0, visible: 0, items: [], callError: r.error || "eval failed" };
    const d = r.data ?? {};
    return "result" in d ? d.result : d;
  };
  const specFor = (t, mode, tag) => ({ steps: stepsOf(t), id: LOC_ID, tag, mode, list: 3 });

  // Ambiguity notes go to the script output: how many visible matches the step saw. At most 4
  // distinct notes per run, then one line saying more were suppressed.
  const NOTE_MAX = 4;
  const noted = new Set();
  let ambiguous = "";
  const ambiguity = (name, t, d) => {
    ambiguous = "";
    if (d.visible <= 1) return;
    ambiguous = ` (the locator matched ${d.visible} visible elements and the first one was used;` +
      " .first()/.nth(i) choose deliberately)";
    const key = name + "|" + describe(t) + "|" + d.visible;
    if (noted.has(key)) return;
    if (noted.size >= NOTE_MAX) {
      if (noted.size === NOTE_MAX) {
        noted.add("more");
        __log("[run note] more ambiguous locators were suppressed");
      }
      return;
    }
    noted.add(key);
    const inAll = d.all > d.visible ? ` (${d.all} in all)` : "";
    __log(
      `[run note] page.${name}(${q(describe(t))}) matched ${d.visible} visible elements${inAll}; ` +
        "acted on the first. Use .first()/.nth(i) to be explicit."
    );
  };

  const listed = (d) =>
    (d.items || []).map((i) => `${i.role || i.tag}${i.name ? " " + JSON.stringify(i.name) : ""}`).join("; ");

  // Resolves until an element can be chosen or `waitMs` has passed. A locator that is invalid
  // (bad selector, bad regular expression) fails at once; a call that cannot be evaluated is
  // retried while the page may still be navigating, then reported as it is.
  const settle = (t, mode, tag, waitMs, fail) => {
    const spec = specFor(t, mode, tag);
    let d = resolveOnce(spec);
    let waited = 0;
    for (;;) {
      if (d.err) fail(`Invalid locator ${describe(t)}: ${d.err}`);
      if (d.picked || mode === "soft") return d;
      if (waited >= waitMs || __remaining() <= 0) break;
      __sleep(250);
      waited += 250;
      d = resolveOnce(spec);
    }
    if (d.callError) fail(`could not evaluate the locator ${describe(t)}: ${d.callError}`);
    return d;
  };

  const T = {
    isLoc,
    needsResolver,
    describe,
    // The CSS selector a command acts on for target `t`. `mode`: "act" (default), "soft" (the
    // caller asks about the element: no waiting, a hidden match is an answer, no match gives null)
    // or "any" (hidden matches count). `fail(message)` throws the step's failure.
    resolve: (t, name, mode, fail, waitMs = WAIT_MS) => {
      if (!needsResolver(t)) return t;
      const how = mode || "act";
      const d = settle(t, how, true, how === "soft" ? 0 : waitMs, fail);
      if (!d.picked) {
        if (how === "soft") return null;
        const wait = `waited ${waitMs / 1000}s`;
        if (d.all === 0) fail(`Element not found: ${describe(t)} (${wait} for it)`);
        fail(
          `Element not visible: ${describe(t)} matched ${d.all} element${d.all === 1 ? "" : "s"} ` +
            `(${listed(d)}) but none is visible or reachable (hidden, display:none, zero-size or ` +
            `collapsed), so nothing was done (${wait}). Open or reveal it first, or read hidden ` +
            "text with page.snapshot({hidden: true})"
        );
      }
      tagged = true;
      last = { t, name, mode: how };
      ambiguity(name, t, d);
      return LOC_SEL;
    },
    // The number of elements a locator matches (its visible ones when it has any), or undefined
    // for a plain selector or ref.
    count: (t, fail) => {
      if (!needsResolver(t)) return undefined;
      return settle(t, "soft", false, 0, fail).n;
    },
    // Polls until `state` holds for the locator or `timeoutMs` passes; returns whether it did.
    until: (t, state, timeoutMs, fail) => {
      const mode = state === "attached" ? "any" : "soft";
      const waitMs = Math.min(timeoutMs, Math.max(0, __remaining()));
      const holds = (d) =>
        state === "visible" ? d.visible > 0
        : state === "attached" ? d.picked
        : state === "hidden" ? d.visible === 0
        : d.all === 0; // detached
      const spec = specFor(t, mode, false);
      let waited = 0;
      for (;;) {
        const d = resolveOnce(spec);
        if (d.err) fail(`Invalid locator ${describe(t)}: ${d.err}`);
        if (!d.callError && holds(d)) return true;
        if (waited >= waitMs) {
          if (d.callError) fail(`could not evaluate the locator ${describe(t)}: ${d.callError}`);
          return false;
        }
        __sleep(250);
        waited += 250;
      }
    },
    isMarker: (sel) => sel === LOC_SEL,
    // The marker vanished between resolving and acting (the page re-rendered the element): resolve
    // the same locator once more. Returns whether an element was found and re-marked.
    reresolve: (sel) => {
      if (sel !== LOC_SEL || !last) return false;
      const d = resolveOnce(specFor(last.t, last.mode, true));
      return !d.err && !d.callError && !!d.picked;
    },
    // Error text shown to the script names the locator the caller wrote, not the marker.
    rename: (message) => {
      const shown = last ? describe(last.t) : "the locator";
      return String(message).split(LOC_SEL).join(shown);
    },
    // Text for a failed step on `sel`: the ambiguity of the locator that was used.
    ambiguity: (sel) => (sel === LOC_SEL ? ambiguous : ""),
    // Runs when the script ends, success or not: removes the markers this run put in the page.
    finish: () => {
      if (!tagged) return;
      // the marker may sit in a shadow root: find it with the page library, not querySelectorAll
      exec(["eval",
        `(() => { for (const o of (${__DOM_LIB}).all(document, ${JSON.stringify(LOC_SEL)})) ` +
          "o.removeAttribute('data-ab-loc'); })()"]);
      tagged = false;
    },
  };
  return T;
})();

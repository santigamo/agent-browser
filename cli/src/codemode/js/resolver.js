// Code mode: the locator resolver. Its source runs in the page (one `eval`), not in QuickJS.
//
// resolver(spec, L) finds the elements a locator describes. `L` is the page library of the CLI
// (native/dom_js.rs): the one shadow-root walker and the one visibility predicate, shared with
// click, fill, wait and get, so a locator and a plain selector see the same elements.
//
// The matches of a locator are its VISIBLE elements when it has any, else all of them. That one
// list is what nth() indexes, what count() counts, what the ambiguity note reports, and what a
// step acts on (its first element), so they cannot disagree.
//
// spec: {steps, id, tag, mode, list}. mode "act" (default): only an ACTIONABLE element is chosen,
// a visible one or a form control a user can really reach; "soft": any match answers (isVisible);
// "any": any match, hidden or not (waiting for attached). With `tag`, the chosen element gets a
// data-ab-loc marker that later steps use as their CSS target.
// Returns {n, visible, picked, how, items, err}; `err` is set when the locator itself is invalid.
const resolver = function (spec, L) {
  const norm = (s) => (s || "").replace(/\s+/g, " ").trim();
  const matcher = (v, exact) => {
    if (v && v.re !== undefined) {
      const r = new RegExp(v.re, v.flags || "");
      return (s) => r.test(norm(s));
    }
    const t = norm(v);
    if (exact) return (s) => norm(s) === t;
    const l = t.toLowerCase();
    return (s) => norm(s).toLowerCase().includes(l);
  };
  // Visible text of an element: innerText excludes <script>/<style> content, textContent does not.
  const txt = (e) => (e.innerText !== undefined ? e.innerText : e.textContent) || "";
  const SKIP = /^(SCRIPT|STYLE|NOSCRIPT|HEAD|META|LINK|TITLE|TEMPLATE|BR)$/;
  // Not display:none / visibility:hidden / aria-hidden: zero-size inputs (visually hidden search
  // boxes) still count.
  const exposed = (el) => {
    if (el.closest && el.closest("[aria-hidden='true']")) return false;
    const s = getComputedStyle(el);
    return s.display !== "none" && s.visibility !== "hidden";
  };
  // The elements a step searches: the root itself when it is an element (a locator chained after
  // a match can match that element's own text), then everything under it, shadow roots included.
  const within = (root) => {
    const under = [root, ...L.roots(root)].flatMap((r) => Array.from(r.querySelectorAll("*")));
    return (root.nodeType === 1 ? [root, ...under] : under).filter((e) => !SKIP.test(e.tagName));
  };
  const labelText = (el) => {
    const ids = (el.getAttribute("aria-labelledby") || "").split(/\s+/).filter(Boolean);
    if (ids.length) {
      const scope = el.getRootNode().getElementById ? el.getRootNode() : document;
      return ids.map((i) => (scope.getElementById(i) || {}).textContent || "").join(" ");
    }
    if (el.getAttribute("aria-label")) return el.getAttribute("aria-label");
    if (el.labels && el.labels.length) return Array.from(el.labels).map(txt).join(" ");
    return "";
  };
  const TAG_ROLE = {
    button: "button", textarea: "textbox", nav: "navigation", main: "main", header: "banner",
    footer: "contentinfo", form: "form", dialog: "dialog", option: "option", article: "article",
    ul: "list", ol: "list", li: "listitem", table: "table", tr: "row", td: "cell",
  };
  const INPUT_ROLE = {
    checkbox: "checkbox", radio: "radio", range: "slider", number: "spinbutton",
    search: "searchbox", hidden: "",
    button: "button", submit: "button", reset: "button", image: "button",
  };
  const roleOf = (el) => {
    const explicit = (el.getAttribute("role") || "").split(/\s+/)[0];
    if (explicit) return explicit;
    const t = el.tagName.toLowerCase();
    const ty = (el.getAttribute("type") || "text").toLowerCase();
    if (t === "a" || t === "area") return el.hasAttribute("href") ? "link" : "";
    if (t === "input") {
      if (ty in INPUT_ROLE) return INPUT_ROLE[ty];
      return el.hasAttribute("list") ? "combobox" : "textbox";
    }
    if (t === "select") return el.multiple || el.size > 1 ? "listbox" : "combobox";
    if (/^h[1-6]$/.test(t)) return "heading";
    if (t === "img") return el.getAttribute("alt") === "" ? "presentation" : "img";
    if (t === "th") return el.getAttribute("scope") === "row" ? "rowheader" : "columnheader";
    return TAG_ROLE[t] || "";
  };
  // Names that live in attributes: icon buttons, search triggers and inputs often have no text.
  const attrNames = (e) => {
    const out = [];
    for (const a of ["aria-label", "title", "alt", "placeholder"]) {
      const v = e.getAttribute && e.getAttribute(a);
      if (v) out.push(v);
    }
    const ty = (e.getAttribute("type") || "").toLowerCase();
    if (e.tagName === "INPUT" && ["button", "submit", "reset"].includes(ty) && e.value) out.push(e.value);
    return out;
  };
  const attrMatch = (e, m) => attrNames(e).some((n) => m(n));
  const nameOf = (el) => {
    const n = labelText(el);
    if (n) return norm(n);
    const t = el.tagName.toLowerCase();
    if (t === "img") return norm(el.getAttribute("alt"));
    if (t === "input") {
      const ty = (el.getAttribute("type") || "text").toLowerCase();
      if (["button", "submit", "reset"].includes(ty)) {
        return norm(el.value || (ty === "submit" ? "Submit" : ""));
      }
      return norm(el.getAttribute("title") || el.getAttribute("placeholder"));
    }
    const own = norm(el.innerText || el.textContent);
    return own || norm(el.getAttribute("title") || el.getAttribute("alt"));
  };
  const uniq = (xs) => Array.from(new Set(xs));
  // The matches of a list of elements: its visible ones when it has any, else all of them.
  const matches = (els) => {
    const vis = els.filter(L.visible);
    return vis.length ? vis : els;
  };
  const attrSelector = (step, attr) => {
    const m = matcher(step.v, !!step.exact);
    return within(document).filter((e) => e.hasAttribute(attr) && m(e.getAttribute(attr)));
  };

  const find = (step, root) => {
    const v = step.v;
    const ex = !!step.exact;
    switch (step.k) {
      case "css":
        return L.all(root, v);
      case "text": {
        const m = matcher(v, ex);
        const all = within(root).filter(L.visible);
        const hits = all.filter((e) => m(txt(e)));
        const deepest = hits.filter((e) => !Array.from(e.children).some((c) => hits.includes(c)));
        const named = all.filter((e) => attrMatch(e, m));
        return all.filter((e) => deepest.includes(e) || named.includes(e));
      }
      case "role": {
        const m = step.name === undefined ? null : matcher(step.name, ex);
        return within(root).filter(
          (e) => roleOf(e) === String(v).toLowerCase() && exposed(e) && (!m || m(nameOf(e)))
        );
      }
      case "label": {
        const m = matcher(v, ex);
        const out = [];
        for (const e of within(root)) {
          if (e.tagName === "LABEL" && m(txt(e))) {
            const forId = e.getAttribute("for");
            const byFor = forId && document.getElementById(forId);
            const c = e.control || byFor || e.querySelector("input,select,textarea");
            if (c) out.push(c);
          } else if (e.hasAttribute("aria-label") && m(e.getAttribute("aria-label"))) out.push(e);
          else if (e.hasAttribute("aria-labelledby") && m(labelText(e))) out.push(e);
          else if (/^(INPUT|SELECT|TEXTAREA)$/.test(e.tagName) && !labelText(e) && attrMatch(e, m)) out.push(e);
        }
        return uniq(out);
      }
      case "placeholder":
        return attrSelector(step, "placeholder");
      case "alt":
        return attrSelector(step, "alt");
      case "title":
        return attrSelector(step, "title");
      case "testid":
        return within(root).filter((e) => e.getAttribute("data-testid") === String(v));
      default:
        throw new Error("unknown selector step " + step.k);
    }
  };

  // Filter by text. A bare filter (no compound before it) applies to every element, and keeps only
  // the DEEPEST matches: ':has-text("x")' means the smallest element that holds the text, not
  // html, body and every wrapper around it. An explicit compound ('li:has-text("x")') keeps all
  // matching elements of that compound.
  const filterText = (cur, step) => {
    const m = matcher(step.v, !!step.exact);
    const sel = "[aria-label],[title],[alt],[placeholder],input[type=button],input[type=submit]";
    const inner = (e) => Array.from(e.querySelectorAll(sel)).some((d) => attrMatch(d, m));
    const ok = (e) => m(txt(e)) || attrMatch(e, m) || inner(e);
    const hits = cur.filter(ok);
    if (!step.bare) return hits;
    return hits.filter((e) => !Array.from(e.querySelectorAll("*")).some((d) => hits.includes(d)));
  };

  let cur = [document];
  let err = "";
  try {
    for (const step of spec.steps) {
      if (step.k === "nth") {
        const list = matches(cur.filter((e) => e.nodeType === 1));
        const i = step.v < 0 ? list.length + step.v : step.v;
        cur = list[i] ? [list[i]] : [];
      } else if (step.k === "has-text") cur = filterText(cur, step);
      else cur = uniq(cur.flatMap((r) => find(step, r)));
    }
  } catch (e) {
    err = String((e && e.message) || e);
    cur = [];
  }

  // Remove this run's old marker wherever it is (light DOM or any open shadow root).
  for (const r of [document, ...L.roots(document)]) {
    for (const o of r.querySelectorAll('[data-ab-loc="' + spec.id + '"]')) o.removeAttribute("data-ab-loc");
  }
  // Actionable = visible, or a form control the user can really reach. A zero-size or display:none
  // checkbox/radio is reached through its visible label (that is what a click on it does); any other
  // visually-hidden control must actually take focus. Anything else is not a target.
  const hiddenControl = (e) => {
    if (!/^(INPUT|TEXTAREA|SELECT)$/.test(e.tagName) || e.disabled || e.type === "hidden") return false;
    if (e.labels && Array.from(e.labels).some(L.visible)) return true;
    if (!exposed(e)) return false;
    try {
      e.focus({ preventScroll: true });
      return e.getRootNode().activeElement === e;
    } catch (_) {
      return false;
    }
  };
  const els = cur.filter((e) => e.nodeType === 1);
  const list = matches(els);
  const vis = els.filter(L.visible);
  let pick = vis[0];
  let how = "visible";
  if (!pick && spec.mode === "act") {
    pick = els.find(hiddenControl);
    how = "hidden-control";
  } else if (!pick && (spec.mode === "soft" || spec.mode === "any")) {
    pick = els[0];
    how = "hidden";
  }
  if (spec.tag && pick) pick.setAttribute("data-ab-loc", spec.id);
  return {
    n: list.length,
    all: els.length,
    visible: vis.length,
    picked: !!pick,
    how: pick ? how : "",
    err,
    items: list.slice(0, spec.list || 0).map((e) => ({
      tag: e.tagName.toLowerCase(),
      role: roleOf(e),
      name: nameOf(e).slice(0, 60),
    })),
  };
};

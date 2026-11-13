// Added in the browser-duel fork of vercel-labs/agent-browser v0.43.0 (Apache-2.0); see CHANGES-FORK.md.
//! Page-side selector helpers shared by every command that takes a CSS or XPath target.
//!
//! One JavaScript library, instantiated inline in the expression that needs it, so that click,
//! fill, get, count, wait and the `run` locator resolver all see the same elements: CSS matches
//! come from the light DOM first, then from open shadow roots (recursively, in document order),
//! and the first match that is visible wins over an earlier hidden one.

use serde_json::to_string as json;

/// A JS expression that evaluates to the helper object `{ roots, all, first, count, xall,
/// visible, hasText }`. It is the only place that walks shadow roots or decides what "visible"
/// means; Rust and the `run` resolver embed it instead of repeating either.
pub(crate) const DOM_LIB_JS: &str = r#"(() => {
  const visible = (e) => {
    const r = e.getBoundingClientRect();
    const s = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  };
  const roots = (r) => {
    const out = [];
    const walk = (n) => { for (const e of n.querySelectorAll('*')) { const sr = e.shadowRoot; if (sr) { out.push(sr); walk(sr); } } };
    walk(r);
    return out;
  };
  const up = (e) => e.parentElement || (e.getRootNode() || {}).host || null;
  const all = (r, s) => {
    const seen = new Set(r.querySelectorAll(s));
    const rs = roots(r);
    for (const sr of rs) for (const e of sr.querySelectorAll(s)) seen.add(e);
    // A descendant selector such as "my-box input" crosses a shadow boundary that
    // querySelectorAll does not: match the last compound inside each shadow root and the rest
    // against the ancestors, hopping from a shadow root to its host.
    if (rs.length && !/[>+~,]/.test(s) && /\s/.test(s.trim())) {
      const parts = s.trim().split(/\s+(?![^(\[]*[)\]])/);
      const last = parts.pop();
      const prefix = parts.join(' ');
      for (const sr of rs) for (const e of sr.querySelectorAll(last)) {
        if (seen.has(e)) continue;
        for (let n = up(e); n; n = up(n)) { if (n.matches && n.matches(prefix)) { seen.add(e); break; } }
      }
    }
    return Array.from(seen);
  };
  const first = (r, s) => {
    const f = r.querySelector(s);
    if (f && visible(f)) return f;
    const a = all(r, s);
    return a.find(visible) || f || a[0] || null;
  };
  const xall = (r, xp) => {
    const snap = (r.ownerDocument || r).evaluate(xp, r, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
    const a = [];
    for (let i = 0; i < snap.snapshotLength; i++) a.push(snap.snapshotItem(i));
    return a;
  };
  // Rendered text only: innerText for the light DOM, and for open shadow roots the text nodes
  // whose element is rendered and is not script, style, noscript or template content.
  const hasText = (t) => {
    if ((document.body.innerText || '').includes(t)) return true;
    for (const sr of roots(document)) {
      const w = document.createTreeWalker(sr, NodeFilter.SHOW_TEXT);
      let buf = '';
      for (let n = w.nextNode(); n; n = w.nextNode()) {
        const p = n.parentNode;
        if (p && p.nodeType === 1 && (p.closest('script,style,noscript,template') || !p.checkVisibility({ visibilityProperty: true }))) continue;
        buf += n.nodeValue;
      }
      if (buf.replace(/\s+/g, ' ').includes(t)) return true;
    }
    return false;
  };
  return { roots, all, first, count: (r, s) => all(r, s).length, xall, visible, hasText };
})()"#;

fn lit(s: &str) -> String {
    json(s).unwrap_or_default()
}

/// Expression for one element: the first visible CSS match under `root` (a JS expression naming a
/// Document or ShadowRoot), or the first XPath match.
pub(crate) fn find_expr(root: &str, selector: &str) -> String {
    match selector.strip_prefix("xpath=") {
        Some(xp) => format!(
            "{root}.evaluate({}, {root}, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue",
            lit(xp)
        ),
        None => format!("{DOM_LIB_JS}.first({root}, {})", lit(selector)),
    }
}

/// Expression for an array of every element matching `selector` under `root`.
pub(crate) fn all_expr(root: &str, selector: &str) -> String {
    match selector.strip_prefix("xpath=") {
        Some(xp) => format!("{DOM_LIB_JS}.xall({root}, {})", lit(xp)),
        None => format!("{DOM_LIB_JS}.all({root}, {})", lit(selector)),
    }
}

/// Expression for the number of elements matching `selector` in the document.
pub(crate) fn count_expr(selector: &str) -> String {
    match selector.strip_prefix("xpath=") {
        Some(xp) => format!(
            "document.evaluate({}, document, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null).snapshotLength",
            lit(xp)
        ),
        None => format!("({}).length", all_expr("document", selector)),
    }
}

/// Check expression for `wait <selector> --state <state>` under `root`. "visible" succeeds when
/// ANY match is visible, because the first match may be a hidden template or a collapsed menu
/// item while a later one is on screen; it is the same element `find_expr` hands to click and fill.
pub(crate) fn state_check_expr(root: &str, selector: &str, state: &str) -> String {
    let all = all_expr(root, selector);
    match state {
        "attached" => format!("(({all}).length > 0)"),
        "detached" => format!("(({all}).length === 0)"),
        "hidden" => format!(
            r#"(() => {{
                const el = {find};
                if (!el) return true;
                const s = (el.ownerDocument.defaultView || window).getComputedStyle(el);
                return s.display === 'none' || s.visibility === 'hidden' || parseFloat(s.opacity) === 0;
            }})()"#,
            find = find_expr(root, selector)
        ),
        _ => format!("(({all}).some({DOM_LIB_JS}.visible))"),
    }
}

/// Check expression for `wait --text`.
pub(crate) fn text_check_expr(text: &str) -> String {
    format!("{DOM_LIB_JS}.hasText({})", lit(text))
}

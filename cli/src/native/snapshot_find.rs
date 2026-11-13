//! `snapshot --find <text>` and `snapshot --hidden-text`: search a rendered snapshot, and read the
//! text a snapshot leaves out (closed `<details>`, hidden elements).

use serde_json::Value;

use super::dom_js::DOM_LIB_JS;

/// Matches `snapshot --find` prints before saying the rest were cut.
pub const FIND_MATCH_LIMIT: usize = 20;

/// Role and name of a rendered tree line, without the indent, the `- ` bullet or the trailing `[ref=..]` attributes.
fn tree_line_text(line: &str) -> &str {
    let t = line.trim_start().trim_start_matches("- ");
    match (t.ends_with(']'), t.rfind(" [")) {
        (true, Some(i)) => &t[..i],
        _ => t,
    }
}

/// Lines of a rendered snapshot whose role or name contains `needle` (case-insensitive), each with its ancestor
/// lines, so a match keeps the context it sits in. The outcome is stated on the first line: how many matches,
/// that nothing matched (and how many lines were searched), or that only the first `limit` of N are shown.
pub fn find_in_tree(tree: &str, needle: &str, limit: usize) -> String {
    let lines: Vec<&str> = tree.lines().filter(|l| !l.trim().is_empty()).collect();
    let indent = |l: &str| l.len() - l.trim_start().len();
    let needle_l = needle.to_lowercase();
    let mut stack: Vec<usize> = Vec::new();
    let mut keep = std::collections::BTreeSet::new();
    let (mut matches, mut shown) = (0usize, 0usize);
    for (i, line) in lines.iter().enumerate() {
        while let Some(&top) = stack.last() {
            if indent(lines[top]) >= indent(line) {
                stack.pop();
            } else {
                break;
            }
        }
        if tree_line_text(line).to_lowercase().contains(&needle_l) {
            matches += 1;
            if shown < limit {
                shown += 1;
                keep.extend(stack.iter().copied());
                keep.insert(i);
            }
        }
        stack.push(i);
    }
    let header = if matches == 0 {
        format!(
            "find {needle:?}: no match in {} snapshot lines (a snapshot lists visible content only; --hidden-text searches closed or hidden content)",
            lines.len()
        )
    } else if matches > shown {
        format!("find {needle:?}: TRUNCATED, showing the first {shown} of {matches} matches (use a longer text or scope with a selector)")
    } else {
        format!(
            "find {needle:?}: {matches} match{}",
            if matches == 1 { "" } else { "es" }
        )
    };
    let mut out = header;
    for i in keep {
        out.push('\n');
        out.push_str(lines[i]);
    }
    out
}

/// In-page walk that collects the text a snapshot omits: closed `<details>` bodies and elements that are
/// `hidden`, `display:none` or `visibility:hidden`. Only the topmost hidden element of a branch is reported,
/// labelled with its container. Open shadow roots come from the shared page library (`dom_js`), and the
/// content of a shadow root whose host sits inside hidden content is left to that host. Returns a JSON
/// string `{pieces:[{c,t}], more}`.
pub fn hidden_text_script(selector: Option<&str>) -> String {
    let sel = serde_json::to_string(&selector).unwrap_or_else(|_| "null".into());
    let lib = DOM_LIB_JS;
    format!(
        r##"(() => {{
  const L = {lib};
  const SEL = {sel};
  const root = SEL ? L.first(document, SEL) : document.body;
  if (!root) return JSON.stringify({{ pieces: [], more: 0, error: "selector matched nothing" }});
  const SKIP = /^(SCRIPT|STYLE|NOSCRIPT|TEMPLATE|HEAD|META|LINK|TITLE)$/;
  const norm = (s) => (s || "").replace(/\s+/g, " ").trim();
  const MAX_PIECES = 60, MAX_CHARS = 2000;
  const pieces = []; let more = 0;
  const tag = (e) => e.tagName.toLowerCase() + (e.id ? "#" + e.id : e.classList && e.classList.length ? "." + e.classList[0] : "");
  const up = (e) => e.parentElement || (e.getRootNode() || {{}}).host || null;
  const path = (e) => {{ const p = []; for (let a = up(e); a && a !== document.body && p.length < 3; a = up(a)) p.unshift(tag(a)); return p.join(" > "); }};
  const push = (e, label, text) => {{
    text = norm(text); if (!text) return;
    if (pieces.length >= MAX_PIECES) {{ more++; return; }}
    const cut = text.length > MAX_CHARS ? " [+" + (text.length - MAX_CHARS) + " chars cut]" : "";
    const p = path(e);
    pieces.push({{ c: (p ? p + " > " : "") + label, t: text.slice(0, MAX_CHARS) + cut }});
  }};
  const why = (e) => {{
    if (e.hidden || e.hasAttribute("hidden")) return "hidden attribute";
    const s = getComputedStyle(e);
    if (s.display === "none") return "display:none";
    if (s.visibility === "hidden" && (!e.parentElement || getComputedStyle(e.parentElement).visibility !== "hidden")) return "visibility:hidden";
    return "";
  }};
  const inHidden = (e) => {{ for (let a = e; a && a !== root; a = up(a)) {{ if (why(a) || (a.tagName === "DETAILS" && !a.open && a !== e)) return true; }} return false; }};
  // Only the children of one tree scope: shadow roots are visited separately through the library.
  const walk = (n) => {{
    for (const e of n.children) {{
      if (SKIP.test(e.tagName)) continue;
      const w = why(e);
      if (w) {{ push(e, "<" + tag(e) + "> (" + w + ")", e.textContent); continue; }}
      if (e.tagName === "DETAILS" && !e.open) {{
        const sum = Array.from(e.children).find((c) => c.tagName === "SUMMARY");
        const body = Array.from(e.childNodes).filter((c) => c !== sum).map((c) => c.textContent).join(" ");
        push(e, '<details "' + norm(sum ? sum.textContent : "").slice(0, 60) + '"> (closed)', body);
        continue;
      }}
      walk(e);
    }}
  }};
  walk(root);
  for (const sr of L.roots(root)) {{ if (!inHidden(sr.host)) walk(sr); }}
  return JSON.stringify({{ pieces, more }});
}})()"##
    )
}

/// Lowercases one character to one character, so positions in the folded text are positions in the
/// original: `str::to_lowercase` can change the length ("İ" becomes two characters), which would
/// make an index into the folded string wrong or not a char boundary of the original.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Index of the first occurrence of `needle` in `hay`, in characters.
fn find_chars(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Renders the pieces `hidden_text_script` found. With `find`, only pieces containing it, trimmed to a window
/// around the first hit; the header says whether nothing was hidden, nothing matched, or the list was cut.
pub fn render_hidden_text(result: &Value, find: Option<&str>) -> String {
    let pieces = result["pieces"].as_array().cloned().unwrap_or_default();
    let more = result["more"].as_u64().unwrap_or(0);
    if let Some(err) = result["error"].as_str() {
        return format!("hidden text: {err}");
    }
    let total = pieces.len();
    let needle: Option<Vec<char>> = find.map(|f| f.chars().map(fold).collect());
    let mut shown: Vec<String> = Vec::new();
    for p in &pieces {
        let (c, t) = (p["c"].as_str().unwrap_or(""), p["t"].as_str().unwrap_or(""));
        let text = match &needle {
            None => t.to_string(),
            Some(n) => {
                let chars: Vec<char> = t.chars().collect();
                let folded: Vec<char> = chars.iter().copied().map(fold).collect();
                match find_chars(&folded, n) {
                    None => continue,
                    Some(at) => {
                        let (from, to) = (
                            at.saturating_sub(200),
                            (at + n.len() + 200).min(chars.len()),
                        );
                        format!(
                            "{}{}{}",
                            if from > 0 { "..." } else { "" },
                            chars[from..to].iter().collect::<String>(),
                            if to < chars.len() { "..." } else { "" }
                        )
                    }
                }
            }
        };
        shown.push(format!("[{c}] {text}"));
    }
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let header = match (&needle, total, shown.len()) {
        (None, 0, _) => "hidden text: none (no closed <details>, hidden, display:none or visibility:hidden content)".to_string(),
        (None, n, _) => format!("hidden text (not visible on the page; each piece is labelled with its container): {n} piece{}", plural(n)),
        (Some(_), 0, _) => "hidden text: none (no hidden content on the page)".to_string(),
        (Some(_), n, 0) if more == 0 => format!("hidden text matching {:?}: no match in {n} hidden piece{}", find.unwrap_or(""), plural(n)),
        (Some(_), n, 0) => format!("hidden text matching {:?}: no match in the first {n} hidden pieces; {more} more were not searched (scope with a selector)", find.unwrap_or("")),
        (Some(_), _, k) => format!("hidden text matching {:?}: {k} piece{}", find.unwrap_or(""), plural(k)),
    };
    let mut out = header;
    for s in shown {
        out.push('\n');
        out.push_str(&s);
    }
    if more > 0 && (needle.is_none() || !out.contains("were not searched")) {
        out.push_str(&format!(
            "\n[TRUNCATED: {more} more hidden pieces not listed; scope with a selector]"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TREE: &str = "- heading \"Report\" [level=1, ref=e1]\n- generic\n  - heading \"Team\" [level=2, ref=e2]\n  - list\n    - listitem [level=1]\n      - StaticText \"Ana\"\n    - listitem [level=1]\n      - StaticText \"Luis \"\n      - link \"profile\" [ref=e3]\n";

    #[test]
    fn find_returns_the_match_with_its_ancestors_only() {
        let out = find_in_tree(TREE, "luis", 20);
        assert_eq!(
            out,
            "find \"luis\": 1 match\n- generic\n  - list\n    - listitem [level=1]\n      - StaticText \"Luis \""
        );
    }

    #[test]
    fn find_tells_no_match_from_truncated() {
        let none = find_in_tree(TREE, "zebra", 20);
        assert!(
            none.starts_with("find \"zebra\": no match in 9 snapshot lines"),
            "{none}"
        );
        assert!(!none.contains("TRUNCATED"));
        let cut = find_in_tree(TREE, "listitem", 1);
        assert!(
            cut.starts_with("find \"listitem\": TRUNCATED, showing the first 1 of 2 matches"),
            "{cut}"
        );
        assert!(!cut.contains("Luis"));
    }

    #[test]
    fn find_ignores_ref_attributes_but_matches_roles() {
        assert!(find_in_tree(TREE, "ref=e3", 20).contains("no match"));
        assert!(find_in_tree(TREE, "LINK", 20).contains("1 match"));
    }

    #[test]
    fn hidden_text_is_labelled_by_container_and_filtered_by_find() {
        let r = json!({"pieces": [
            {"c": "main > <details \"Shipping rules\"> (closed)", "t": "Orders above the threshold ship free in March."},
            {"c": "<div#tpl> (hidden attribute)", "t": "Promo text"}], "more": 0});
        let all = render_hidden_text(&r, None);
        assert!(all.starts_with("hidden text (not visible on the page; each piece is labelled with its container): 2 pieces"), "{all}");
        assert!(
            all.contains("[main > <details \"Shipping rules\"> (closed)] Orders above"),
            "{all}"
        );
        let hit = render_hidden_text(&r, Some("THRESHOLD"));
        assert!(
            hit.starts_with("hidden text matching \"THRESHOLD\": 1 piece")
                && !hit.contains("Promo"),
            "{hit}"
        );
        assert!(render_hidden_text(&r, Some("zzz")).contains("no match in 2 hidden pieces"));
        assert!(render_hidden_text(&json!({"pieces": [], "more": 0}), None)
            .starts_with("hidden text: none"));
        let cut = render_hidden_text(&json!({"pieces": [{"c": "x", "t": "a"}], "more": 5}), None);
        assert!(cut.contains("[TRUNCATED: 5 more hidden pieces"), "{cut}");
    }

    #[test]
    fn matching_does_not_slice_by_the_byte_offsets_of_lowercased_text() {
        // "İ" lowercases to two characters and "ß" stays one: byte offsets of the lowercased text
        // are not offsets of the original, which used to panic or cut inside a character.
        let t = format!("{}needle after", "İß€".repeat(200));
        let r = json!({"pieces": [{"c": "x", "t": t}], "more": 0});
        let out = render_hidden_text(&r, Some("NEEDLE"));
        assert!(out.contains("needle after") && out.contains("..."), "{out}");
        let multi =
            json!({"pieces": [{"c": "x", "t": "日本語のテキスト Needle 終わり"}], "more": 0});
        assert!(render_hidden_text(&multi, Some("needle")).contains("Needle 終わり"));
    }

    #[test]
    fn hidden_script_embeds_the_selector_safely_and_reuses_the_shared_library() {
        let s = hidden_text_script(Some("#a\"b"));
        assert!(s.contains(r##"const SEL = "#a\"b";"##), "{s}");
        assert!(hidden_text_script(None).contains("const SEL = null;"));
        // shadow roots come from the shared library, not a second walker
        assert!(s.contains("L.roots(root)") && !s.contains("e.shadowRoot) walk"));
    }
}

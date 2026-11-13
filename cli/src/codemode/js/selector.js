// Added in the browser-duel fork of vercel-labs/agent-browser v0.43.0 (Apache-2.0); see CHANGES-FORK.md.
// Code mode: selector strings.
//
// A target is a CSS selector, an @ref, or a Playwright-style selector that this file turns into
// steps for the resolver: "text=Foo", 'text="Foo"', 'role=button[name="Go"]', "css=a",
// 'a:has-text("Go")', "a >> text=Next", "nth=1". Plain CSS and @refs never go through here.

// Steps are objects: {k: "css" | "text" | "role" | "label" | "placeholder" | "alt" | "title" |
// "testid" | "nth" | "has-text", v: ..., exact?: bool, name?: ...}.
const ENGINE = /^(text|role|css|nth|label|placeholder|alt|title|testid)=|:has-text\(|:text(-is)?\(|>>/;

// Splits "a >> b" at ">>", but not inside a quoted string: 'a:has-text("x >> y")' is one part.
const splitChain = (str) => {
  const parts = [];
  let cur = "";
  let quote = "";
  for (let i = 0; i < str.length; i++) {
    const c = str[i];
    if (quote) {
      cur += c;
      if (c === "\\" && i + 1 < str.length) cur += str[++i];
      else if (c === quote) quote = "";
    } else if (c === '"' || c === "'") {
      quote = c;
      cur += c;
    } else if (c === ">" && str[i + 1] === ">") {
      parts.push(cur);
      cur = "";
      i++;
    } else cur += c;
  }
  parts.push(cur);
  return parts;
};

// "Foo" / 'Foo' (exact), /re/flags (regular expression), or bare text (case-insensitive substring).
const unquote = (s) => {
  s = s.trim();
  const m = /^(["'])(.*)\1(i?)$/s.exec(s);
  if (m) return [m[2], true];
  const r = /^\/(.*)\/([a-z]*)$/s.exec(s);
  if (r) return [{ re: r[1], flags: r[2] }, false];
  return [s, false];
};

const ROLE_SEL = /^role=([\w-]+)(?:\[name=(?:"([^"]*)"|'([^']*)'|\/(.*?)\/([a-z]*))(i)?\])?$/s;
const TEXT_ENGINES = ["text", "label", "placeholder", "alt", "title"];

// CSS with :has-text("...") / :text("...") / :text-is("..."): the filter applies to the compound it
// is attached to; whatever follows is searched inside the matching elements
// ("li:has-text('x') .toggle"). A filter with no compound before it applies to every element.
const parseCss = (part, steps) => {
  const re = /:(has-text|text-is|text)\((["'])(.*?)\2\)/gs;
  const pushCss = (c) => {
    c = c.trim();
    if (c !== "") steps.push({ k: "css", v: /^[>+~]/.test(c) ? ":scope " + c : c });
  };
  let last = 0;
  let found = false;
  for (let m = re.exec(part); m; m = re.exec(part)) {
    found = true;
    const before = part.slice(last, m.index);
    const bare = before.trim() === "";
    pushCss(bare ? "*" : before);
    steps.push({ k: "has-text", v: m[3], exact: m[1] === "text-is", bare });
    last = re.lastIndex;
  }
  if (!found) steps.push({ k: "css", v: part });
  else pushCss(part.slice(last));
};

const parseSel = (str) => {
  const steps = [];
  for (const raw of splitChain(str)) {
    const part = raw.trim();
    let m;
    const engine = TEXT_ENGINES.find((e) => part.startsWith(e + "="));
    if (engine) {
      const [v, exact] = unquote(part.slice(engine.length + 1));
      steps.push({ k: engine, v, exact });
    } else if (part.startsWith("testid=")) steps.push({ k: "testid", v: part.slice(7) });
    else if ((m = /^nth=(-?\d+)$/.exec(part))) steps.push({ k: "nth", v: Number(m[1]) });
    else if (part.startsWith("css=")) steps.push({ k: "css", v: part.slice(4) });
    else if ((m = ROLE_SEL.exec(part))) {
      const step = { k: "role", v: m[1] };
      const nm = m[2] ?? m[3];
      if (nm !== undefined) {
        step.name = nm;
        step.exact = !m[6];
      } else if (m[4] !== undefined) step.name = { re: m[4], flags: m[5] };
      steps.push(step);
    } else parseCss(part, steps);
  }
  return steps;
};

// Added in the browser-duel fork of vercel-labs/agent-browser v0.43.0 (Apache-2.0); see CHANGES-FORK.md.
// Code mode: Playwright-style locator objects (page.locator, page.getByText, ...).
//
// A locator is a list of steps (selector.js) plus a description for messages. It resolves when a
// step uses it, never before; every method hands it to the page API, which resolves it (see
// targets.js). Options that a locator call does not support are an error.
const L = (() => {
  const rx = (v) => (v instanceof RegExp ? { re: v.source, flags: v.flags } : v);
  const only = page.__only;
  const text = (v) => (v instanceof RegExp ? String(v) : String(v));
  const roleDesc = (role, o) =>
    `role=${role}${o.name !== undefined ? `[name=${JSON.stringify(text(o.name))}]` : ""}`;

  // The steps a getBy* call adds, with the options each one supports.
  const getBy = {
    getByText: (v, o) => ({ steps: [{ k: "text", v: rx(v), exact: !!o.exact }], desc: `text=${text(v)}` }),
    getByLabel: (v, o) => ({ steps: [{ k: "label", v: rx(v), exact: !!o.exact }], desc: `label=${text(v)}` }),
    getByPlaceholder: (v, o) => ({
      steps: [{ k: "placeholder", v: rx(v), exact: !!o.exact }], desc: `placeholder=${text(v)}`,
    }),
    getByAltText: (v, o) => ({ steps: [{ k: "alt", v: rx(v), exact: !!o.exact }], desc: `alt=${text(v)}` }),
    getByTitle: (v, o) => ({ steps: [{ k: "title", v: rx(v), exact: !!o.exact }], desc: `title=${text(v)}` }),
    getByTestId: (v) => ({ steps: [{ k: "testid", v: String(v) }], desc: `testid=${v}` }),
    getByRole: (role, o) => ({
      steps: [{ k: "role", v: role, name: o.name === undefined ? undefined : rx(o.name), exact: !!o.exact }],
      desc: roleDesc(role, o),
    }),
  };
  const GET_BY_OPTS = {
    getByText: ["exact"], getByLabel: ["exact"], getByPlaceholder: ["exact"], getByAltText: ["exact"],
    getByTitle: ["exact"], getByTestId: [], getByRole: ["name", "exact"],
  };
  const build = (base, kind, v, opts) => {
    const o = only(opts, GET_BY_OPTS[kind], `${kind}()`);
    const made = getBy[kind](v, o);
    return base ? { steps: [...base.steps, ...made.steps], desc: `${base.desc} >> ${made.desc}` } : made;
  };

  const make = (steps, desc) => {
    const l = {
      __loc: true,
      steps,
      desc,
      locator: (s) => make([...steps, ...(page.__steps(s))], `${desc} >> ${page.__describe(s)}`),
      nth: (i) => make([...steps, { k: "nth", v: i }], `${desc} >> nth=${i}`),
      first: () => l.nth(0),
      last: () => l.nth(-1),
      // Only hasText filters; has, hasNot, hasNotText and the rest are errors, not no-ops.
      filter: (opts) => {
        const o = only(opts, ["hasText"], "locator.filter()");
        if (o.hasText === undefined) return l;
        return make([...steps, { k: "has-text", v: rx(o.hasText) }], `${desc}.filter({hasText: ${text(o.hasText)}})`);
      },
      count: () => page.count(l),
      all: () => Array.from({ length: page.count(l) }, (_, i) => l.nth(i)),
      click: (o) => page.click(l, o),
      dblclick: (o) => page.dblclick(l, o),
      hover: (o) => page.hover(l, o),
      focus: (o) => page.focus(l, o),
      check: (o) => page.check(l, o),
      uncheck: (o) => page.uncheck(l, o),
      fill: (v, o) => page.fill(l, v, o),
      type: (v, o) => page.type(l, v, o),
      selectOption: (...v) => page.select(l, ...v),
      text: (o) => page.text(l, o),
      innerText: (o) => page.text(l, o),
      textContent: (o) => page.text(l, o),
      html: (o) => page.html(l, o),
      inputValue: (o) => page.value(l, o),
      getAttribute: (n, o) => page.attr(l, n, o),
      isVisible: (o) => page.isVisible(l, o),
      // waitFor({state: "visible" | "hidden" | "attached" | "detached", timeout})
      waitFor: (opts) => {
        const o = only(opts, ["state", "timeout"], "locator.waitFor()");
        return page.waitFor({ selector: l, state: o.state, timeout: o.timeout });
      },
    };
    for (const kind of Object.keys(getBy)) {
      l[kind] = (v, o) => {
        const b = build({ steps, desc }, kind, v, o);
        return make(b.steps, b.desc);
      };
    }
    return l;
  };
  for (const kind of Object.keys(getBy)) {
    page[kind] = (v, o) => {
      const b = build(null, kind, v, o);
      return make(b.steps, b.desc);
    };
  }
  page.locator = (s) => make(page.__steps(s), page.__describe(s));
  return make;
})();

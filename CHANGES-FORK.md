# Changes in this fork

This is a fork of [vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser) v0.43.0, versioned `0.43.0-duel.1`. It is not the official package and is not supported by Vercel. The license is unchanged (Apache-2.0, see `LICENSE`). Under section 4(b) of that license, every modified source file carries a notice at its top, and this file lists every modified file and every changed default.

The fork was made for the browser-duel comparison. Its changes are ported from an earlier measured fork of v0.41.2, with the review's defects fixed. Nothing here has been proposed upstream.

## Restoring upstream behavior

Set `AGENT_BROWSER_UPSTREAM_DEFAULTS` to a comma separated list of the names below, or to `all`. The CLI reads it for the command it runs; the daemon reads it when it starts, so set it before the first command of a session or run `agent-browser close` first.

<table>
  <thead>
    <tr><th>Changed default</th><th>Upstream behavior</th><th>Name</th></tr>
  </thead>
  <tbody>
    <tr><td><code>batch</code> stops at the first failing command, prints <code>[n/total]</code> before each result, and reads one command per line from stdin</td><td>Continues after a failure. <code>--bail</code> is still accepted. The <code>[n/total]</code> header and line input are additions with no switch</td><td><code>batch</code></td></tr>
    <tr><td>A terminal caps page text (<code>read</code>, <code>get text</code>, <code>get html</code>, <code>eval</code>) at 12000 characters, keeping head and tail. Piped output and <code>--json</code> are not capped. With <code>--max-output</code> the text keeps head and tail instead of only its head</td><td>No default cap. Use <code>--max-output 0</code> for no limit. Head-and-tail for an explicit <code>--max-output</code> has no switch</td><td><code>text-cap</code></td></tr>
    <tr><td><code>open</code> without <code>--wait-until</code> returns about 3 s after DOMContentLoaded with a <code>loadWarning</code> note when <code>load</code> never fires but the document is parsed</td><td>Fails after the 25 s load timeout. An explicit <code>--wait-until load</code> is strict in both</td><td><code>open</code></td></tr>
    <tr><td>A <code>wait</code> on a selector or text with no timeout of the caller's own fails after about 3 s when the document is complete, unchanged for 1 s and no request is in flight</td><td>Waits the full 25 s. <code>--timeout</code>, <code>AGENT_BROWSER_DEFAULT_TIMEOUT</code> and the config default timeout are always waited out</td><td><code>wait-fail-fast</code></td></tr>
    <tr><td><code>wait --url</code> returns after the page has loaded (up to 3 s more)</td><td>Returns when the URL matches</td><td><code>wait-url</code></td></tr>
    <tr><td><code>snapshot -i</code> lists the first 300 items and counts the rest; <code>--all</code> lifts the cap</td><td>Lists everything</td><td><code>snapshot-cap</code></td></tr>
    <tr><td><code>click</code> on a hidden checkbox or radio clicks its visible label (or the input itself) and prints a note</td><td>Fails as not visible</td><td><code>hidden-input</code></td></tr>
    <tr><td>The MCP default profile <code>core</code> leads with <code>agent_browser_run</code>, <code>snapshot</code> and <code>batch</code>; single-step tools are in the new <code>actions</code> profile</td><td>The upstream <code>core</code> list, single-step tools and no <code>run</code></td><td><code>mcp-profile</code></td></tr>
  </tbody>
</table>

Changes with no switch, because they are additions or fixes rather than changed defaults:

- CSS targets search open shadow roots after the light DOM and prefer a visible match over an earlier hidden one, for click, fill, get, count and wait (`cli/src/native/dom_js.rs`). `wait --text` ignores style, script and hidden text.
- `fill` sets date, time, datetime-local, month, week, color and range inputs by value, checks the result, and rejects disabled and read-only inputs. Snapshots show a date or time input as one textbox.
- `snapshot -i` omits the repeated accessible name of table rows and the cells that only wrap their links. `snapshot` gains `--find`, `--hidden-text` and `--all`.
- `--snapshot` (after a successful action, or after the last batch command) and `--offset` are new global flags. `open` accepts `--wait-until`.
- Click and press wait for a navigation they start when asked to settle (`--snapshot`, and every `run` step).
- `agent-browser run <file|->` and the `agent_browser_run` MCP tool are new, behind the `codemode` cargo feature, which is on by default. Build with `--no-default-features` for a binary without them.
- MCP: the `minimal` and `actions` profiles, instructions built from the enabled tools, shorter shared argument descriptions, `snapshot` arguments `find`, `hiddenText` and `all`, `batch` `bail` defaulting to true, and `--max-output`, `--offset` and `--snapshot` allowed in `extraArgs`. Results keep their upstream shape.
- Agent-facing texts: `skill-data/core/SKILL.md` leads with the `run` workflow, and `skills/agent-browser/SKILL.md` is a short self-sufficient stub (see Agent-facing text changes).
- Version `0.43.0-duel.1` in `package.json`, `cli/Cargo.toml`, `cli/Cargo.lock`, the dashboard and the platform packages (by `scripts/sync-version.js`).
- The README opens with a banner that says this is a fork.

Upstream's flow rewrite (#2397) is not touched; the earlier fork's flow changes were dropped.

## Modified and added files

Source files modified from upstream (each starts with the Apache-2.0 section 4(b) notice):

- `cli/src/commands.rs`: `run`, `wait --soft-timeout`, `wait` fail-fast flag, `snapshot --find --hidden-text --all`, `open --wait-until`, `batch --keep-going`.
- `cli/src/connection.rs`: a thread-local request deadline for `run`, no retries past it.
- `cli/src/flags.rs`: `--offset`, `--snapshot`.
- `cli/src/main.rs`: batch loop (`[n/total]`, stdin lines, `--snapshot`, stop at first error), `run` dispatch, `--snapshot` for single commands.
- `cli/src/mcp.rs`: the MCP changes above.
- `cli/src/output.rs`: head and tail cap, `--help` text, `run` help.
- `cli/src/native/actions.rs`: bounded navigation, wait fail-fast with the in-flight request set, `wait --url` settling, settle after click and press, hidden checkbox click, `snapshot` find and hidden text.
- `cli/src/native/browser.rs`: the bounded `load` wait.
- `cli/src/native/element.rs`: selector expressions delegate to `dom_js`.
- `cli/src/native/interaction.rs`: `fill` for structured inputs, hidden checkbox click.
- `cli/src/native/mod.rs`: new modules.
- `cli/src/native/stream/chat.rs`: a test that keeps `run` out of the chat allowlist.
- `cli/src/native/snapshot.rs`: date and time collapse, item cap, row and cell squashing.
- `cli/src/native/e2e_tests.rs`: Chrome-gated tests for the above.
- `cli/tests/batch_cli.rs`: tests for the new batch default.

Source files added by the fork:

- `cli/src/fork.rs`: the `AGENT_BROWSER_UPSTREAM_DEFAULTS` switch.
- `cli/src/native/dom_js.rs`: the page-side selector library (shadow roots, visibility, text).
- `cli/src/native/snapshot_find.rs`: `snapshot --find` and `--hidden-text`.
- `cli/src/codemode/mod.rs`, `cli/src/codemode/tests.rs`, `cli/src/codemode/js/*.js`: `run`.
- `cli/tests/run_cli.rs`: `run` and its locators against a real page.

Other files modified:

- `README.md`, `CHANGES-FORK.md` (new): the fork banner and this list.
- `package.json`, `cli/Cargo.toml`, `cli/Cargo.lock`, `packages/dashboard/package.json`, `packages/@agent-browser/*/package.json`, `packages/@agent-browser/sandbox/src/version.ts`: version, plus `rquickjs` as an optional dependency and the `codemode` feature in the Cargo files.
- `packages/@agent-browser/sandbox/mcp-tools.json`: the regenerated MCP tool catalog.
- `docs/content/docs/commands.mdx`, `skill-data/core/references/commands.md`, `skill-data/core/SKILL.md`, `skills/agent-browser/SKILL.md`: documentation and agent-facing texts.

## Agent-facing text changes

These are the texts an agent reads: the core guide (`agent-browser skills get core`), the stub, the MCP `initialize` instructions and tool descriptions, and `--help`.

- `skill-data/core/SKILL.md`: leads with the `run` workflow (one script per page state, the page API, targets and locators, what to do after the first miss, when to snapshot, `batch` for a few ref actions). It states only what the code does: printed output is not capped by default, a selector or text wait without a timeout of its own gives up after about 3 s on a settled page, a chosen timeout is waited out, and unsupported options throw. It keeps upstream's routing table.
- `skills/agent-browser/SKILL.md`: deliberately a self-sufficient stub (the `run` workflow, targets and failure handling in under 2800 bytes), against upstream's `AGENTS.md`, which asks for a thin pointer to `agent-browser skills get core`. A pointer stub makes the agent spend its first step loading the guide before it works; a stub that already carries the workflow does not. Every claim in it is checked against this fork's code.
- MCP instructions: built from the enabled tools. With `agent_browser_run` enabled they lead with the one-script workflow; without it they keep the upstream wording. A tool is named only when its profile enables it.
- MCP descriptions: `open`, `snapshot`, `batch` and the shared arguments are shorter and point at `agent_browser_run`.
- `--help`: new text for `run`, `--snapshot`, `--offset`, `--max-output`, `batch`, `fill`, `click`, `open`, `wait` and `snapshot`.

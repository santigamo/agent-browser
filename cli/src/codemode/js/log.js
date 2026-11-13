// Added in the browser-duel fork of vercel-labs/agent-browser v0.43.0 (Apache-2.0); see CHANGES-FORK.md.
// Code mode, part 1 of 3: console capture and the script's log buffer.
//
// console.log is capped while it is collected, not only when the output is printed: a loop that
// logs on every iteration cannot grow without bound inside the script. Past the cap, lines are
// counted, not kept, so the kept log stays a prefix of what was logged.
globalThis.__logs = [];
let __logChars = 0;
let __logDropped = 0;
let __logMark = -1;

globalThis.__log = (line) => {
  const cap = typeof __logCap === "number" ? __logCap : 12000;
  if (__logMark < 0 && __logChars + line.length + 1 <= cap) {
    __logChars += line.length + 1;
    __logs.push(line);
    return;
  }
  __logDropped++;
  if (__logMark < 0) {
    __logMark = __logs.length;
    __logs.push("");
  }
  const s = __logDropped === 1 ? "" : "s";
  __logs[__logMark] = `[console.log cap reached: ${__logDropped} further log line${s} dropped]`;
};

const __show = (x) => (typeof x === "string" ? x : JSON.stringify(x));
globalThis.console = { log: (...a) => __log(a.map(__show).join(" ")) };
console.info = console.warn = console.error = console.debug = console.log;

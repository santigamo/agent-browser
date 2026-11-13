// Code mode, part 2 of 3: what a step's target may be.
//
// A target is a CSS selector or an @ref, passed to the command as it is. Everything that touches
// targets goes through `T`, so the page API below does not care how a target is resolved.
const T = {
  // The selector the underlying command acts on. `soft`: the caller asks about the element
  // (isVisible), so a hidden match is an answer, not an error.
  resolve: (target, _name, _soft) => target,
  // How the target is shown in a failure message.
  describe: (target) => String(target),
  isLoc: (_target) => false,
  // The number of elements the target matches, or undefined to ask the daemon.
  count: (_target) => undefined,
  // Extra text for a failed step on `selector`, or "".
  ambiguity: (_selector) => "",
  // Runs when the script ends, success or not.
  finish: () => {},
};

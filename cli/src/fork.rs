//! Switches for the browser-duel fork's changed defaults.
//!
//! The fork changes a few defaults of upstream agent-browser (see CHANGES-FORK.md). Each one can
//! be turned back to the upstream behaviour with `AGENT_BROWSER_UPSTREAM_DEFAULTS`, a comma
//! separated list of names, or `all`. The daemon reads the variable when it starts, so set it
//! before the first command of a session (or run `agent-browser close` first).

/// True when the named fork default is switched back to upstream behaviour.
pub fn restores(name: &str) -> bool {
    match std::env::var("AGENT_BROWSER_UPSTREAM_DEFAULTS") {
        Ok(v) => restores_in(&v, name),
        Err(_) => false,
    }
}

fn restores_in(list: &str, name: &str) -> bool {
    list.split(',')
        .map(str::trim)
        .any(|item| item == "all" || item == name)
}

#[cfg(test)]
mod tests {
    use super::restores_in;

    #[test]
    fn names_are_matched_exactly_and_all_matches_everything() {
        assert!(restores_in("all", "batch"));
        assert!(restores_in("cap, batch", "batch"));
        assert!(!restores_in("batch-x", "batch"));
        assert!(!restores_in("", "batch"));
    }
}

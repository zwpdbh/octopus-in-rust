pub struct HookEngine;

impl HookEngine {
    pub fn new() -> Self {
        Self
    }

    /// Trigger a hook by name.
    ///
    /// `matcher_value` and `error_message` are used by hook matchers to decide
    /// whether the hook should fire.
    ///
    /// Returns the number of handlers that were invoked (for now always 0
    /// since hooks are not yet implemented).
    pub fn trigger(
        &self,
        _name: &str,
        _matcher_value: String,
        _error_message: String,
    ) -> usize {
        // TODO: implement real hook engine with async handler dispatch,
        // matcher evaluation, and action results.
        0
    }
}

impl Default for HookEngine {
    fn default() -> Self {
        Self::new()
    }
}

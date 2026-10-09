use crate::*;

impl RuntimeEngine {
    /// Reads the user-defined persona (the `persona` app setting, edited from
    /// Settings -> Persona in the app) and renders it as a system block.
    /// A missing, blank, or unreadable setting yields an empty string, which the
    /// request builders skip, so an unset persona changes nothing about the prompt.
    pub(crate) fn build_persona_system_prompt(&self) -> String {
        let raw = self
            .database
            .app_setting("persona")
            .ok()
            .flatten()
            .unwrap_or_default();
        format_persona_system_prompt(&raw)
    }
}

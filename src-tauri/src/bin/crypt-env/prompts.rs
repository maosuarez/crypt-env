use dialoguer::Confirm;

/// Shows a yes/no prompt. Returns false on error (safe default).
pub fn confirm(message: &str) -> bool {
    Confirm::new()
        .with_prompt(message)
        .default(false)
        .interact()
        .unwrap_or(false)
}

use clap::Args;
use crate::client::{CliError, find_and_reveal};
use crate::shell::{detect_shell, format_assignment, verify_hint, Shell};

#[derive(Args)]
pub struct InjectArgs {
    /// Name of the secret to inject
    pub name: String,

    /// Force shell format: pwsh, bash, zsh, sh
    #[arg(long)]
    pub shell: Option<String>,
}

pub fn run(args: InjectArgs) -> Result<(), CliError> {
    let (_, value) = find_and_reveal(&args.name)?;

    let shell = match args.shell.as_deref() {
        Some("pwsh") | Some("powershell") => Shell::PowerShell,
        Some("bash") => Shell::Bash,
        Some("zsh") => Shell::Zsh,
        Some("sh") => Shell::Sh,
        _ => detect_shell(),
    };

    // Print assignment to stdout — clean for pipe-eval
    // The value is embedded in a safely single-quoted shell assignment
    println!("{}", format_assignment(&shell, &args.name, &value));

    // Print verification hint to stderr — does not interfere with piped eval
    eprintln!("# Verify: {}", verify_hint(&shell, &args.name));

    Ok(())
}

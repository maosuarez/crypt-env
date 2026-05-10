use clap::Args;
use crate::client::{CliError, find_and_reveal};
use crate::shell::{detect_shell, format_assignment};

#[derive(Args)]
pub struct SetArgs {
    /// Name of the secret
    pub name: String,
}

pub fn run(args: SetArgs) -> Result<(), CliError> {
    let (_, value) = find_and_reveal(&args.name)?;
    let shell = detect_shell();
    // Value is embedded in the shell assignment string (single-quoted, safely escaped)
    println!("{}", format_assignment(&shell, &args.name, &value));
    Ok(())
}

use clap::Args;
use crate::client::{CliError, find_and_reveal};
use crate::commands::scope;
use crate::shell::{detect_shell, format_assignment};

#[derive(Args)]
pub struct SetArgs {
    /// Name of the secret
    pub name: String,

    /// Project name (defaults to crypt-env.json or the cwd folder name)
    #[arg(long)]
    pub project: Option<String>,

    /// Environment name (defaults to crypt-env.json or the project's default environment)
    #[arg(long = "env")]
    pub env: Option<String>,
}

pub fn run(args: SetArgs) -> Result<(), CliError> {
    let resolved_scope = scope::resolve(args.project.as_deref(), args.env.as_deref(), false)?;
    let (_, value) = find_and_reveal(&args.name, &resolved_scope.to_query_string())?;
    let shell = detect_shell();
    // Value is embedded in the shell assignment string (single-quoted, safely escaped)
    println!("{}", format_assignment(&shell, &args.name, &value));
    Ok(())
}

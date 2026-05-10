use clap::Args;
use std::path::{Path, PathBuf};
use crate::client::{CliError, find_and_reveal};

#[derive(Args)]
pub struct FillArgs {
    /// Path to .env or .env.example. Defaults to current directory.
    pub path: Option<PathBuf>,
}

pub fn run(args: FillArgs) -> Result<(), CliError> {
    let source_path = resolve_env_path(args.path.as_deref())?;
    let is_example = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.ends_with(".example"))
        .unwrap_or(false);

    let output_path = if is_example {
        // .env.example → create sibling .env
        source_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(".env")
    } else {
        source_path.clone()
    };

    let content = std::fs::read_to_string(&source_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let mut new_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut injected = 0usize;
    let mut not_found = 0usize;

    for line in &lines {
        let trimmed = line.trim();

        // Preserve comments and blank lines unchanged
        if trimmed.is_empty() || trimmed.starts_with('#') {
            new_lines.push(line.to_string());
            continue;
        }

        if let Some(eq_pos) = trimmed.find('=') {
            let key = &trimmed[..eq_pos];

            // Only process valid identifiers
            if key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                match find_and_reveal(key) {
                    Ok((_, value)) => {
                        new_lines.push(format!("{}={}", key, value));
                        injected += 1;
                        continue;
                    }
                    Err(CliError::NotFound(_)) => {
                        eprintln!("warning: '{}' not found in vault", key);
                        not_found += 1;
                        if is_example {
                            new_lines.push(format!("{}=", key));
                        } else {
                            new_lines.push(line.to_string());
                        }
                        continue;
                    }
                    Err(CliError::VaultLocked) => return Err(CliError::VaultLocked),
                    Err(e) => return Err(e),
                }
            }
        }

        new_lines.push(line.to_string());
    }

    let output = new_lines.join("\n");
    let output = if content.ends_with('\n') {
        format!("{}\n", output)
    } else {
        output
    };

    std::fs::write(&output_path, output)?;
    eprintln!(
        "OK: {} ({} secrets injected, {} not found)",
        output_path.display(),
        injected,
        not_found
    );
    eprintln!(
        "Warning: '{}' contains secrets in plaintext. Keep permissions restrictive.",
        output_path.display()
    );
    Ok(())
}

fn resolve_env_path(arg: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(p) = arg {
        return Ok(p.to_path_buf());
    }
    let cwd = std::env::current_dir()?;
    for name in &[".env", ".env.example"] {
        let candidate = cwd.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(CliError::NotFound(
        ".env or .env.example in current directory".to_string(),
    ))
}

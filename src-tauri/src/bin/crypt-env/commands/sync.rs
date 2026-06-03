use clap::Args;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use crate::client::{CliError, find_and_reveal};

#[derive(Args)]
pub struct SyncArgs {
    /// Path to .env.example (source). Defaults to .env.example in current dir.
    #[arg(long)]
    pub example: Option<PathBuf>,

    /// Path to .env (target). Defaults to .env in same dir as .env.example.
    #[arg(long)]
    pub env: Option<PathBuf>,

    /// Only show what would be added, without writing.
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: SyncArgs) -> Result<(), CliError> {
    let example_path = resolve_example_path(args.example.as_deref())?;

    let env_path = match args.env {
        Some(p) => p,
        None => example_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(".env"),
    };

    let existing_keys = parse_existing_keys(&env_path)?;

    let example_content = std::fs::read_to_string(&example_path).map_err(|e| {
        CliError::Io(std::io::Error::new(
            e.kind(),
            format!("Cannot read {}: {}", example_path.display(), e),
        ))
    })?;

    let mut lines_to_add: Vec<String> = Vec::new();
    let mut from_vault = 0usize;
    let mut blank_count = 0usize;

    for line in example_content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(eq_pos) = trimmed.find('=') {
            let key = &trimmed[..eq_pos];

            if !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }

            if existing_keys.contains(key) {
                continue;
            }

            let new_line = match find_and_reveal(key) {
                Ok((_, value)) => {
                    from_vault += 1;
                    format!("{}={}", key, value)
                }
                Err(CliError::NotFound(_)) => {
                    blank_count += 1;
                    format!("{}=", key)
                }
                Err(CliError::VaultLocked) => return Err(CliError::VaultLocked),
                Err(e) => return Err(e),
            };

            lines_to_add.push(new_line);
        }
    }

    let total = from_vault + blank_count;

    if args.dry_run {
        if lines_to_add.is_empty() {
            eprintln!("Dry run: no new variables found in {}", example_path.display());
        } else {
            eprintln!(
                "Dry run: would add {} variable(s) to {} ({} from vault, {} blank):",
                total,
                env_path.display(),
                from_vault,
                blank_count,
            );
            for line in &lines_to_add {
                // Print key name only — never expose values in dry-run output.
                let key = line.split('=').next().unwrap_or(line);
                eprintln!("  + {}", key);
            }
        }
        return Ok(());
    }

    if lines_to_add.is_empty() {
        eprintln!(
            "OK: {} is already up to date with {}",
            env_path.display(),
            example_path.display()
        );
        return Ok(());
    }

    let append_content = build_append_block(&lines_to_add, env_path.exists());
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&env_path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(append_content.as_bytes())
        })?;

    eprintln!(
        "Synced {} new variable(s) to {} ({} from vault, {} blank)",
        total,
        env_path.display(),
        from_vault,
        blank_count,
    );
    if from_vault > 0 {
        eprintln!(
            "Warning: '{}' contains secrets in plaintext. Keep permissions restrictive.",
            env_path.display()
        );
    }

    Ok(())
}

fn resolve_example_path(arg: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(p) = arg {
        if p.exists() {
            return Ok(p.to_path_buf());
        }
        return Err(CliError::NotFound(format!(
            "No .env.example found at {}",
            p.display()
        )));
    }

    let cwd = std::env::current_dir()?;
    let candidate = cwd.join(".env.example");
    if candidate.exists() {
        return Ok(candidate);
    }

    Err(CliError::NotFound(
        "No .env.example found at .env.example in current directory".to_string(),
    ))
}

fn parse_existing_keys(env_path: &Path) -> Result<HashSet<String>, CliError> {
    if !env_path.exists() {
        return Ok(HashSet::new());
    }

    let content = std::fs::read_to_string(env_path)?;
    let mut keys = HashSet::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(eq_pos) = trimmed.find('=') {
            let key = &trimmed[..eq_pos];
            if key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                keys.insert(key.to_string());
            }
        }
    }

    Ok(keys)
}

fn build_append_block(lines: &[String], file_exists: bool) -> String {
    let mut block = String::new();
    if file_exists {
        block.push('\n');
    }
    for line in lines {
        block.push_str(line);
        block.push('\n');
    }
    block
}

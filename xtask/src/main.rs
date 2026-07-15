use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use xtask::{
    GenerationOutcome, SyncOutcome, generate_notices, generate_provider, sync_git_grammars,
    sync_grammars, verify_architecture, verify_git_grammars, verify_grammars,
};

enum GrammarSource {
    Directory(PathBuf),
    Git { repository: PathBuf, prefix: String },
}

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[String]) -> Result<String, String> {
    if arguments == ["architecture", "verify"] {
        let report = verify_architecture().map_err(|error| error.to_string())?;
        return Ok(format!(
            "verified {} packages / {} internal dependencies / {} migration exceptions",
            report.packages, report.dependencies, report.exceptions
        ));
    }
    if arguments.len() < 2 || arguments[0] != "grammars" {
        return Err(usage());
    }
    let command = arguments[1].as_str();
    let options = parse_options(&arguments[2..])?;
    let lock = required_path(&options, "--lock")?;
    match command {
        "verify" => {
            reject_unknown(
                &options,
                &["--lock", "--source", "--git-repo", "--git-prefix"],
            )?;
            let verified = match grammar_source(&options)? {
                GrammarSource::Directory(source) => {
                    verify_grammars(lock, source).map_err(|error| error.to_string())?
                }
                GrammarSource::Git { repository, prefix } => {
                    verify_git_grammars(lock, repository, &prefix)
                        .map_err(|error| error.to_string())?
                }
            };
            Ok(format!(
                "verified {} grammars / {} assets",
                verified.grammar_count, verified.asset_count
            ))
        }
        "sync" => {
            reject_unknown(
                &options,
                &["--lock", "--source", "--git-repo", "--git-prefix", "--dest"],
            )?;
            let destination = required_path(&options, "--dest")?;
            let outcome = match grammar_source(&options)? {
                GrammarSource::Directory(source) => {
                    sync_grammars(lock, source, destination).map_err(|error| error.to_string())?
                }
                GrammarSource::Git { repository, prefix } => {
                    sync_git_grammars(lock, repository, &prefix, destination)
                        .map_err(|error| error.to_string())?
                }
            };
            Ok(match outcome {
                SyncOutcome::Created => "grammar pack materialized".into(),
                SyncOutcome::AlreadyCurrent => "grammar pack already current".into(),
            })
        }
        "generate-provider" | "generate-notices" => {
            reject_unknown(&options, &["--lock", "--output", "--check"])?;
            let output = required_path(&options, "--output")?;
            let check = options.contains_key("--check");
            let outcome = if command == "generate-provider" {
                generate_provider(lock, output, check).map_err(|error| error.to_string())?
            } else {
                generate_notices(lock, output, check).map_err(|error| error.to_string())?
            };
            Ok(match (command, outcome) {
                ("generate-provider", GenerationOutcome::Written) => {
                    "full grammar provider generated".into()
                }
                ("generate-notices", GenerationOutcome::Written) => {
                    "full grammar notices generated".into()
                }
                ("generate-provider", GenerationOutcome::Unchanged) => {
                    "full grammar provider is current".into()
                }
                ("generate-notices", GenerationOutcome::Unchanged) => {
                    "full grammar notices are current".into()
                }
                _ => unreachable!("matched generator command"),
            })
        }
        _ => Err(usage()),
    }
}

fn parse_options(arguments: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut options = BTreeMap::new();
    let mut index = 0;
    while index < arguments.len() {
        let option = &arguments[index];
        if !option.starts_with("--") {
            return Err(usage());
        }
        if option == "--check" {
            if options.insert(option.clone(), "true".into()).is_some() {
                return Err(format!("duplicate option {option}"));
            }
            index += 1;
            continue;
        }
        let value = arguments.get(index + 1).ok_or_else(usage)?;
        if value.starts_with("--") {
            return Err(usage());
        }
        if options.insert(option.clone(), value.clone()).is_some() {
            return Err(format!("duplicate option {option}"));
        }
        index += 2;
    }
    Ok(options)
}

fn required_path(options: &BTreeMap<String, String>, key: &str) -> Result<PathBuf, String> {
    options
        .get(key)
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing {key}; {}", usage()))
}

fn grammar_source(options: &BTreeMap<String, String>) -> Result<GrammarSource, String> {
    match (
        options.get("--source"),
        options.get("--git-repo"),
        options.get("--git-prefix"),
    ) {
        (Some(source), None, None) => Ok(GrammarSource::Directory(PathBuf::from(source))),
        (None, Some(repository), Some(prefix)) => Ok(GrammarSource::Git {
            repository: PathBuf::from(repository),
            prefix: prefix.clone(),
        }),
        _ => Err(format!(
            "choose exactly one grammar source mode: --source <dir> or paired --git-repo <dir> --git-prefix <path>; {}",
            usage()
        )),
    }
}

fn reject_unknown(options: &BTreeMap<String, String>, allowed: &[&str]) -> Result<(), String> {
    if let Some(option) = options.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("unknown option {option}; {}", usage()));
    }
    Ok(())
}

fn usage() -> String {
    "usage: cargo xtask architecture verify | cargo xtask grammars verify --lock <file> (--source <dir> | --git-repo <dir> --git-prefix <path>) | cargo xtask grammars sync --lock <file> (--source <dir> | --git-repo <dir> --git-prefix <path>) --dest <dir> | cargo xtask grammars generate-provider --lock <file> --output <file> [--check] | cargo xtask grammars generate-notices --lock <file> --output <file> [--check]".into()
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn rejects_mixed_directory_and_git_source_modes() {
        let arguments = [
            "grammars",
            "verify",
            "--lock",
            "lock.toml",
            "--source",
            "source",
            "--git-repo",
            "repository",
            "--git-prefix",
            "vendor/grammars",
        ]
        .map(str::to_owned);

        let error = run(&arguments).unwrap_err();

        assert!(error.contains("exactly one grammar source mode"), "{error}");
    }

    #[test]
    fn rejects_missing_or_incomplete_source_modes() {
        let cases = [
            vec!["--lock", "lock.toml"],
            vec!["--lock", "lock.toml", "--git-repo", "repository"],
            vec!["--lock", "lock.toml", "--git-prefix", "vendor/grammars"],
        ];

        for options in cases {
            let mut arguments = vec!["grammars".to_owned(), "verify".to_owned()];
            arguments.extend(options.into_iter().map(str::to_owned));

            let error = run(&arguments).unwrap_err();

            assert!(error.contains("exactly one grammar source mode"), "{error}");
        }
    }
}

use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use xtask::{SyncOutcome, sync_grammars, verify_grammars};

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
    if arguments.len() < 2 || arguments[0] != "grammars" {
        return Err(usage());
    }
    let command = arguments[1].as_str();
    let options = parse_options(&arguments[2..])?;
    let lock = required_path(&options, "--lock")?;
    let source = required_path(&options, "--source")?;
    match command {
        "verify" => {
            reject_unknown(&options, &["--lock", "--source"])?;
            let verified = verify_grammars(lock, source).map_err(|error| error.to_string())?;
            Ok(format!(
                "verified {} grammars / {} assets",
                verified.grammar_count, verified.asset_count
            ))
        }
        "sync" => {
            reject_unknown(&options, &["--lock", "--source", "--dest"])?;
            let destination = required_path(&options, "--dest")?;
            let outcome =
                sync_grammars(lock, source, destination).map_err(|error| error.to_string())?;
            Ok(match outcome {
                SyncOutcome::Created => "grammar pack materialized".into(),
                SyncOutcome::AlreadyCurrent => "grammar pack already current".into(),
            })
        }
        _ => Err(usage()),
    }
}

fn parse_options(arguments: &[String]) -> Result<BTreeMap<String, String>, String> {
    if !arguments.len().is_multiple_of(2) {
        return Err(usage());
    }
    let mut options = BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        if !pair[0].starts_with("--") || pair[1].starts_with("--") {
            return Err(usage());
        }
        if options.insert(pair[0].clone(), pair[1].clone()).is_some() {
            return Err(format!("duplicate option {}", pair[0]));
        }
    }
    Ok(options)
}

fn required_path(options: &BTreeMap<String, String>, key: &str) -> Result<PathBuf, String> {
    options
        .get(key)
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing {key}; {}", usage()))
}

fn reject_unknown(options: &BTreeMap<String, String>, allowed: &[&str]) -> Result<(), String> {
    if let Some(option) = options.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("unknown option {option}; {}", usage()));
    }
    Ok(())
}

fn usage() -> String {
    "usage: cargo xtask grammars verify --lock <file> --source <dir> | cargo xtask grammars sync --lock <file> --source <dir> --dest <dir>".into()
}

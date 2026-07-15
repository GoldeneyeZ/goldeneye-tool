use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use goldeneye_artifact::{
    ArtifactQuality, artifact_commit, artifact_exists, export_artifact, import_artifact,
};
use goldeneye_bootstrap::BootstrapRuntime;
use goldeneye_http::{BoundServer, GoldeneyeBackend, ServerConfig};
use goldeneye_index::canonical_project;
use goldeneye_services::ServiceConfig;
use serde_json::json;

const USAGE: &str = "Usage:\n  goldeneye\n  goldeneye --version\n  goldeneye ui [--bind <address>] [--base-path <path>]\n  goldeneye artifact export <repository> [fast|best]\n  goldeneye artifact import <repository>\n  goldeneye artifact exists <repository>\n  goldeneye artifact commit <repository>";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    match args.first().and_then(|value| value.to_str()) {
        None => run_stdio(),
        Some("--version" | "-V") if args.len() == 1 => {
            println!("goldeneye {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h") if args.len() == 1 => {
            println!("{USAGE}");
            Ok(())
        }
        Some("artifact") => run_artifact(&args[1..]),
        Some("ui") => run_ui(&args[1..]),
        _ => Err(invalid_arguments().into()),
    }
}

fn run_stdio() -> Result<(), Box<dyn std::error::Error>> {
    goldeneye::run_session(std::io::stdin().lock(), std::io::stdout().lock())
}

fn run_ui(args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = ServerConfig::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].to_str() {
            Some("--bind") => {
                let address = args
                    .get(index + 1)
                    .and_then(|value| value.to_str())
                    .ok_or_else(invalid_arguments)?;
                config.bind_address = address.parse()?;
                index += 2;
            }
            Some("--base-path") => {
                let base_path = args
                    .get(index + 1)
                    .and_then(|value| value.to_str())
                    .ok_or_else(invalid_arguments)?;
                config = config.with_base_path(base_path)?;
                index += 2;
            }
            _ => return Err(invalid_arguments().into()),
        }
    }

    let base_path = config.base_path.clone();
    let service_config = ServiceConfig::from_env()?;
    let runtime = BootstrapRuntime::from_config(service_config);
    let server = BoundServer::bind(config, GoldeneyeBackend::with_runtime(runtime))?;
    let address = server.local_addr()?;
    println!("http://{address}{base_path}/");
    server.run()?;
    Ok(())
}

fn run_artifact(args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(command) = args.first().and_then(|value| value.to_str()) else {
        return Err(invalid_arguments().into());
    };
    let repository = repository_argument(args)?;
    match command {
        "export" if args.len() <= 3 => {
            let quality = quality_argument(args.get(2))?;
            let config = ServiceConfig::from_env()?;
            let project = canonical_project(&repository)?;
            let exported = export_artifact(
                config.database_path(),
                &repository,
                project.id.as_str(),
                quality,
            )?;
            println!(
                "{}",
                json!({
                    "artifact_path": exported.artifact_path,
                    "metadata_path": exported.metadata_path,
                    "metadata": exported.metadata,
                })
            );
        }
        "import" if args.len() == 2 => {
            let config = ServiceConfig::from_env()?;
            let imported = import_artifact(&repository, config.database_path())?;
            println!(
                "{}",
                json!({
                    "database_path": imported.database_path,
                    "metadata": imported.metadata,
                })
            );
        }
        "exists" if args.len() == 2 => {
            println!("{}", json!({ "exists": artifact_exists(&repository) }));
        }
        "commit" if args.len() == 2 => {
            println!("{}", json!({ "commit": artifact_commit(&repository)? }));
        }
        _ => return Err(invalid_arguments().into()),
    }
    Ok(())
}

fn repository_argument(args: &[OsString]) -> Result<PathBuf, io::Error> {
    args.get(1).map(PathBuf::from).ok_or_else(invalid_arguments)
}

fn quality_argument(value: Option<&OsString>) -> Result<ArtifactQuality, io::Error> {
    match value.and_then(|value| value.to_str()) {
        None | Some("fast") => Ok(ArtifactQuality::Fast),
        Some("best") => Ok(ArtifactQuality::Best),
        Some(_) => Err(invalid_arguments()),
    }
}

fn invalid_arguments() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, USAGE)
}

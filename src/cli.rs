use crate::importer::{import_source, ImportConfig, ImportError};
use crate::kicad_table::ensure_project_tables;
use crate::kicad_sym::AddPolicy;
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

const DEFAULT_SYMBOL_LIB: &str = "project_symbols.kicad_sym";
const DEFAULT_FOOTPRINT_LIB: &str = "project_footprints.pretty";
const DEFAULT_STEP_DIR: &str = "project_3d";

#[derive(Parser, Debug)]
#[command(name = "kci", version, about = "KiCad component importer")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Import(ImportArgs),
}

#[derive(Args, Debug)]
pub struct ImportArgs {
    #[arg(value_name = "SOURCE")]
    pub source: PathBuf,
    #[arg(long, value_name = "SYMBOL_LIB")]
    pub symbol_lib: Option<PathBuf>,
    #[arg(long, value_name = "FOOTPRINT_LIB")]
    pub footprint_lib: Option<PathBuf>,
    #[arg(long, value_name = "STEP_DIR")]
    pub step_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigFile {
    #[serde(default)]
    symbol_lib: Option<PathBuf>,
    #[serde(default)]
    footprint_lib: Option<PathBuf>,
    #[serde(default)]
    step_dir: Option<PathBuf>,
}

impl ConfigFile {
    fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }

    fn write(&self, path: &Path) -> Result<(), ConfigError> {
        let data = toml::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    fn from_import_config(config: &ImportConfig) -> Self {
        Self {
            symbol_lib: Some(config.symbol_lib().to_path_buf()),
            footprint_lib: Some(config.footprint_lib().to_path_buf()),
            step_dir: Some(config.step_dir().to_path_buf()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportPlan {
    source: PathBuf,
    config: ImportConfig,
    config_path: PathBuf,
    created_config: bool,
}

impl ImportPlan {
    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn config(&self) -> &ImportConfig {
        &self.config
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn created_config(&self) -> bool {
        self.created_config
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Parse(toml::de::Error),
    Write(toml::ser::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(err) => write!(f, "io error: {}", err),
            ConfigError::Parse(err) => write!(f, "config parse error: {}", err),
            ConfigError::Write(err) => write!(f, "config write error: {}", err),
        }
    }
}

impl Error for ConfigError {}

impl From<io::Error> for ConfigError {
    fn from(value: io::Error) -> Self {
        ConfigError::Io(value)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(value: toml::de::Error) -> Self {
        ConfigError::Parse(value)
    }
}

impl From<toml::ser::Error> for ConfigError {
    fn from(value: toml::ser::Error) -> Self {
        ConfigError::Write(value)
    }
}

#[derive(Debug)]
pub enum CliError {
    Config(ConfigError),
    Import(ImportError),
    Table(crate::kicad_table::TableError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Config(err) => write!(f, "{}", err),
            CliError::Import(err) => write!(f, "{}", err),
            CliError::Table(err) => write!(f, "{}", err),
        }
    }
}

impl Error for CliError {}

impl From<ConfigError> for CliError {
    fn from(value: ConfigError) -> Self {
        CliError::Config(value)
    }
}

impl From<ImportError> for CliError {
    fn from(value: ImportError) -> Self {
        CliError::Import(value)
    }
}

impl From<crate::kicad_table::TableError> for CliError {
    fn from(value: crate::kicad_table::TableError) -> Self {
        CliError::Table(value)
    }
}

pub fn resolve_import(args: ImportArgs, cwd: &Path) -> Result<ImportPlan, ConfigError> {
    let config_path = cwd.join(".kci_config");
    let config_file = if config_path.exists() {
        Some(ConfigFile::load(&config_path)?)
    } else {
        None
    };

    let defaults = default_config(cwd);

    let symbol_lib = resolve_path(
        &args.symbol_lib,
        config_file
            .as_ref()
            .and_then(|config| config.symbol_lib.as_ref()),
        defaults.symbol_lib(),
    );
    let footprint_lib = resolve_path(
        &args.footprint_lib,
        config_file
            .as_ref()
            .and_then(|config| config.footprint_lib.as_ref()),
        defaults.footprint_lib(),
    );
    let step_dir = resolve_path(
        &args.step_dir,
        config_file.as_ref().and_then(|config| config.step_dir.as_ref()),
        defaults.step_dir(),
    );

    let config = ImportConfig::new(symbol_lib, footprint_lib, step_dir);

    let mut created_config = false;
    if config_file.is_none() {
        let file = ConfigFile::from_import_config(&config);
        file.write(&config_path)?;
        created_config = true;
    }

    Ok(ImportPlan {
        source: args.source,
        config,
        config_path,
        created_config,
    })
}

fn default_config(cwd: &Path) -> ImportConfig {
    if let Some(project_name) = project_name_from_kicad_pro(cwd) {
        return ImportConfig::new(
            PathBuf::from(format!("{}_symbols.kicad_sym", project_name)),
            PathBuf::from(format!("{}_footprints.pretty", project_name)),
            PathBuf::from(format!("{}_step", project_name)),
        );
    }
    ImportConfig::new(
        PathBuf::from(DEFAULT_SYMBOL_LIB),
        PathBuf::from(DEFAULT_FOOTPRINT_LIB),
        PathBuf::from(DEFAULT_STEP_DIR),
    )
}

fn resolve_path(
    cli_value: &Option<PathBuf>,
    config_value: Option<&PathBuf>,
    default: &Path,
) -> PathBuf {
    if let Some(value) = cli_value {
        return value.clone();
    }
    if let Some(value) = config_value {
        return value.clone();
    }
    default.to_path_buf()
}

fn project_name_from_kicad_pro(cwd: &Path) -> Option<String> {
    let mut names = Vec::new();
    let dir_name = cwd.file_name().and_then(|value| value.to_str());
    let entries = std::fs::read_dir(cwd).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("kicad_pro") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
            names.push(stem.to_string());
        }
    }
    if names.is_empty() {
        return None;
    }
    if let Some(dir_name) = dir_name {
        if names.iter().any(|name| name == dir_name) {
            return Some(dir_name.to_string());
        }
    }
    names.sort();
    names.first().cloned()
}

pub fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Import(args) => {
            let cwd = std::env::current_dir().map_err(ConfigError::from)?;
            let plan = resolve_import(args, &cwd)?;
            let report = import_source(plan.source(), plan.config(), AddPolicy::ReplaceExisting)?;
            ensure_project_tables(&cwd, plan.config())?;
            if plan.created_config() {
                println!("wrote config to {}", plan.config_path().display());
            }
            println!(
                "imported {} symbols, {} footprints, {} step files",
                report.symbols_added(),
                report.footprints_added(),
                report.step_files_added()
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_import_creates_default_config() {
        let dir = tempdir().unwrap();
        let args = ImportArgs {
            source: dir.path().join("source.zip"),
            symbol_lib: None,
            footprint_lib: None,
            step_dir: None,
        };
        let plan = resolve_import(args, dir.path()).unwrap();
        assert!(plan.created_config());
        assert_eq!(plan.config().symbol_lib(), Path::new(DEFAULT_SYMBOL_LIB));
        assert_eq!(plan.config().footprint_lib(), Path::new(DEFAULT_FOOTPRINT_LIB));
        assert_eq!(plan.config().step_dir(), Path::new(DEFAULT_STEP_DIR));
        let stored = ConfigFile::load(plan.config_path()).unwrap();
        assert_eq!(stored.symbol_lib.as_ref().unwrap(), Path::new(DEFAULT_SYMBOL_LIB));
        assert_eq!(stored.footprint_lib.as_ref().unwrap(), Path::new(DEFAULT_FOOTPRINT_LIB));
        assert_eq!(stored.step_dir.as_ref().unwrap(), Path::new(DEFAULT_STEP_DIR));
    }

    #[test]
    fn resolve_import_uses_kicad_pro_name_for_defaults() {
        let dir = tempdir().unwrap();
        let pro_path = dir.path().join("my_project.kicad_pro");
        std::fs::write(&pro_path, "dummy").unwrap();
        let args = ImportArgs {
            source: dir.path().join("source.zip"),
            symbol_lib: None,
            footprint_lib: None,
            step_dir: None,
        };
        let plan = resolve_import(args, dir.path()).unwrap();
        assert!(plan.created_config());
        assert_eq!(
            plan.config().symbol_lib(),
            Path::new("my_project_symbols.kicad_sym")
        );
        assert_eq!(
            plan.config().footprint_lib(),
            Path::new("my_project_footprints.pretty")
        );
        assert_eq!(plan.config().step_dir(), Path::new("my_project_step"));
    }

    #[test]
    fn resolve_import_uses_partial_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join(".kci_config");
        std::fs::write(&config_path, "symbol_lib = \"sym.kicad_sym\"\n").unwrap();
        let args = ImportArgs {
            source: dir.path().join("source.zip"),
            symbol_lib: None,
            footprint_lib: None,
            step_dir: None,
        };
        let plan = resolve_import(args, dir.path()).unwrap();
        assert!(!plan.created_config());
        assert_eq!(plan.config().symbol_lib(), Path::new("sym.kicad_sym"));
        assert_eq!(plan.config().footprint_lib(), Path::new(DEFAULT_FOOTPRINT_LIB));
        assert_eq!(plan.config().step_dir(), Path::new(DEFAULT_STEP_DIR));
    }

    #[test]
    fn resolve_import_cli_overrides_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join(".kci_config");
        std::fs::write(
            &config_path,
            "symbol_lib = \"sym.kicad_sym\"\nfootprint_lib = \"foot.pretty\"\nstep_dir = \"steps\"\n",
        )
        .unwrap();
        let args = ImportArgs {
            source: dir.path().join("source.zip"),
            symbol_lib: Some(PathBuf::from("override.kicad_sym")),
            footprint_lib: None,
            step_dir: Some(PathBuf::from("override_steps")),
        };
        let plan = resolve_import(args, dir.path()).unwrap();
        assert_eq!(plan.config().symbol_lib(), Path::new("override.kicad_sym"));
        assert_eq!(plan.config().footprint_lib(), Path::new("foot.pretty"));
        assert_eq!(plan.config().step_dir(), Path::new("override_steps"));
    }
}

use crate::kicad_sym::{AddPolicy, KicadSymError, KicadSymbolLib, Symbol};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Debug, Clone)]
pub struct ImportConfig {
    symbol_lib: PathBuf,
    footprint_lib: PathBuf,
    step_dir: PathBuf,
}

impl ImportConfig {
    pub fn new(symbol_lib: PathBuf, footprint_lib: PathBuf, step_dir: PathBuf) -> Self {
        Self {
            symbol_lib,
            footprint_lib,
            step_dir,
        }
    }

    pub fn symbol_lib(&self) -> &Path {
        &self.symbol_lib
    }

    pub fn footprint_lib(&self) -> &Path {
        &self.footprint_lib
    }

    pub fn step_dir(&self) -> &Path {
        &self.step_dir
    }
}

#[derive(Debug, Clone)]
pub struct ImportReport {
    symbols_added: usize,
    footprints_added: usize,
    step_files_added: usize,
}

impl ImportReport {
    pub fn symbols_added(&self) -> usize {
        self.symbols_added
    }

    pub fn footprints_added(&self) -> usize {
        self.footprints_added
    }

    pub fn step_files_added(&self) -> usize {
        self.step_files_added
    }
}

#[derive(Debug)]
pub enum ImportError {
    Io(io::Error),
    Symbol(KicadSymError),
    Zip(zip::result::ZipError),
    Walkdir(walkdir::Error),
    InvalidSource(String),
    MissingSymbols,
    MissingFootprints,
    Association(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io(err) => write!(f, "io error: {}", err),
            ImportError::Symbol(err) => write!(f, "symbol parse error: {}", err),
            ImportError::Zip(err) => write!(f, "zip error: {}", err),
            ImportError::Walkdir(err) => write!(f, "walk error: {}", err),
            ImportError::InvalidSource(msg) => write!(f, "invalid source: {}", msg),
            ImportError::MissingSymbols => write!(f, "no symbols found in source"),
            ImportError::MissingFootprints => write!(f, "no footprints found in source"),
            ImportError::Association(msg) => write!(f, "association error: {}", msg),
        }
    }
}

impl Error for ImportError {}

impl From<io::Error> for ImportError {
    fn from(value: io::Error) -> Self {
        ImportError::Io(value)
    }
}

impl From<KicadSymError> for ImportError {
    fn from(value: KicadSymError) -> Self {
        ImportError::Symbol(value)
    }
}

impl From<zip::result::ZipError> for ImportError {
    fn from(value: zip::result::ZipError) -> Self {
        ImportError::Zip(value)
    }
}

impl From<walkdir::Error> for ImportError {
    fn from(value: walkdir::Error) -> Self {
        ImportError::Walkdir(value)
    }
}

pub fn import_source(
    source: &Path,
    config: &ImportConfig,
    policy: AddPolicy,
) -> Result<ImportReport, ImportError> {
    let source_ctx = SourceContext::open(source)?;
    let symbol_files = find_files(&source_ctx.root, "kicad_sym")?;
    if symbol_files.is_empty() {
        return Err(ImportError::MissingSymbols);
    }
    let footprint_files = find_files(&source_ctx.root, "kicad_mod")?;
    if footprint_files.is_empty() {
        return Err(ImportError::MissingFootprints);
    }
    let step_files = find_step_files(&source_ctx.root)?;

    let mut symbols = Vec::new();
    for path in &symbol_files {
        let content = fs::read_to_string(path)?;
        let lib = KicadSymbolLib::parse(&content)?;
        for symbol in lib.symbols()? {
            symbols.push(symbol);
        }
    }

    let footprint_infos = collect_footprints(&footprint_files)?;
    let footprint_lib_name = footprint_lib_name(config.footprint_lib())?;
    let symbols = associate_footprints(symbols, &footprint_infos, &footprint_lib_name)?;

    let symbols_added = symbols.len();
    let mut target_lib = load_or_create_symbol_lib(config.symbol_lib())?;
    for symbol in symbols {
        target_lib.add_symbol(symbol, policy)?;
    }
    if let Some(parent) = config.symbol_lib().parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(config.symbol_lib(), target_lib.to_string_pretty())?;

    let footprints_added = copy_footprints(&footprint_infos, config.footprint_lib())?;
    let step_files_added = copy_steps(&step_files, config.step_dir())?;

    Ok(ImportReport {
        symbols_added,
        footprints_added,
        step_files_added,
    })
}

fn load_or_create_symbol_lib(path: &Path) -> Result<KicadSymbolLib, ImportError> {
    if path.exists() {
        let content = fs::read_to_string(path)?;
        Ok(KicadSymbolLib::parse(&content)?)
    } else {
        let content = "(kicad_symbol_lib (version 20231120))";
        Ok(KicadSymbolLib::parse(content)?)
    }
}

struct SourceContext {
    root: PathBuf,
    _temp: Option<TempDir>,
}

impl SourceContext {
    fn open(path: &Path) -> Result<Self, ImportError> {
        if path.is_dir() {
            return Ok(Self {
                root: path.to_path_buf(),
                _temp: None,
            });
        }
        if is_zip(path) {
            let temp = TempDir::new()?;
            extract_zip(path, temp.path())?;
            return Ok(Self {
                root: temp.path().to_path_buf(),
                _temp: Some(temp),
            });
        }
        Err(ImportError::InvalidSource(format!(
            "expected directory or .zip: {}",
            path.display()
        )))
    }
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), ImportError> {
    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let out_path = match entry.enclosed_name() {
            Some(path) => dest.join(path),
            None => continue,
        };
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out_file = fs::File::create(&out_path)?;
        io::copy(&mut entry, &mut out_file)?;
    }
    Ok(())
}

fn find_files(root: &Path, extension: &str) -> Result<Vec<PathBuf>, ImportError> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if has_extension(path, extension) {
            out.push(path.to_path_buf());
        }
    }
    Ok(out)
}

fn find_step_files(root: &Path) -> Result<Vec<PathBuf>, ImportError> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if has_extension(path, "step") || has_extension(path, "stp") {
            out.push(path.to_path_buf());
        }
    }
    Ok(out)
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case(ext))
        .unwrap_or(false)
}

fn is_zip(path: &Path) -> bool {
    has_extension(path, "zip")
}

#[derive(Clone, Debug)]
struct FootprintInfo {
    name: String,
    path: PathBuf,
}

fn collect_footprints(paths: &[PathBuf]) -> Result<Vec<FootprintInfo>, ImportError> {
    let mut out = Vec::new();
    for path in paths {
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                ImportError::InvalidSource(format!(
                    "invalid footprint filename: {}",
                    path.display()
                ))
            })?
            .to_string();
        out.push(FootprintInfo {
            name,
            path: path.to_path_buf(),
        });
    }
    Ok(out)
}

fn associate_footprints(
    symbols: Vec<Symbol>,
    footprints: &[FootprintInfo],
    footprint_lib_name: &str,
) -> Result<Vec<Symbol>, ImportError> {
    let mut out = Vec::with_capacity(symbols.len());
    let mut footprints_by_name = HashMap::new();
    for footprint in footprints {
        footprints_by_name.insert(footprint.name.as_str(), footprint);
    }

    for mut symbol in symbols {
        let footprint_name =
            select_footprint_for_symbol(&symbol, &footprints_by_name, footprints.len())?;
        let value = format!("{}:{}", footprint_lib_name, footprint_name);
        symbol.set_or_add_property("Footprint", &value);
        out.push(symbol);
    }
    Ok(out)
}

fn select_footprint_for_symbol(
    symbol: &Symbol,
    footprints_by_name: &HashMap<&str, &FootprintInfo>,
    footprint_count: usize,
) -> Result<String, ImportError> {
    if let Some(value) = symbol.property_value("Footprint") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            if let Some(name) = footprint_name_from_value(trimmed) {
                if footprints_by_name.contains_key(name) {
                    return Ok(name.to_string());
                }
            }
        }
    }
    if footprint_count == 1 {
        if let Some((name, _)) = footprints_by_name.iter().next() {
            return Ok((*name).to_string());
        }
    }
    if footprints_by_name.contains_key(symbol.name()) {
        return Ok(symbol.name().to_string());
    }
    Err(ImportError::Association(format!(
        "unable to choose footprint for symbol {}",
        symbol.name()
    )))
}

fn footprint_name_from_value(value: &str) -> Option<&str> {
    if value.is_empty() {
        return None;
    }
    if let Some((_, name)) = value.rsplit_once(':') {
        return Some(name);
    }
    Some(value)
}

fn footprint_lib_name(path: &Path) -> Result<String, ImportError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| ImportError::InvalidSource("invalid footprint lib path".to_string()))?;
    if let Some(stripped) = name.strip_suffix(".pretty") {
        if stripped.is_empty() {
            return Err(ImportError::InvalidSource(
                "invalid footprint lib name".to_string(),
            ));
        }
        return Ok(stripped.to_string());
    }
    Ok(name.to_string())
}

fn copy_footprints(
    footprints: &[FootprintInfo],
    dest_lib: &Path,
) -> Result<usize, ImportError> {
    fs::create_dir_all(dest_lib)?;
    let mut count = 0;
    for footprint in footprints {
        let file_name = footprint
            .path
            .file_name()
            .ok_or_else(|| ImportError::InvalidSource("invalid footprint path".to_string()))?;
        let dest_path = dest_lib.join(file_name);
        fs::copy(&footprint.path, &dest_path)?;
        count += 1;
    }
    Ok(count)
}

fn copy_steps(step_files: &[PathBuf], dest_dir: &Path) -> Result<usize, ImportError> {
    if step_files.is_empty() {
        return Ok(0);
    }
    fs::create_dir_all(dest_dir)?;
    let mut count = 0;
    for step in step_files {
        let file_name = step
            .file_name()
            .ok_or_else(|| ImportError::InvalidSource("invalid step path".to_string()))?;
        let dest_path = dest_dir.join(file_name);
        fs::copy(step, dest_path)?;
        count += 1;
    }
    Ok(count)
}

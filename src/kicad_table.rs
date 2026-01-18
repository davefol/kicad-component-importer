use crate::importer::ImportConfig;
use crate::kicad_sym::{parse_one, Atom, Sexp};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
enum TableKind {
    Symbol,
    Footprint,
}

impl TableKind {
    fn root_name(self) -> &'static str {
        match self {
            TableKind::Symbol => "sym_lib_table",
            TableKind::Footprint => "fp_lib_table",
        }
    }
}

#[derive(Debug)]
pub enum TableError {
    Io(io::Error),
    Parse(String),
    Invalid(String),
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableError::Io(err) => write!(f, "io error: {}", err),
            TableError::Parse(err) => write!(f, "table parse error: {}", err),
            TableError::Invalid(err) => write!(f, "table error: {}", err),
        }
    }
}

impl Error for TableError {}

impl From<io::Error> for TableError {
    fn from(value: io::Error) -> Self {
        TableError::Io(value)
    }
}

pub fn ensure_project_tables(
    project_root: &Path,
    config: &ImportConfig,
) -> Result<(), TableError> {
    ensure_table(
        &project_root.join("sym-lib-table"),
        TableKind::Symbol,
        project_root,
        config.symbol_lib(),
    )?;
    ensure_table(
        &project_root.join("fp-lib-table"),
        TableKind::Footprint,
        project_root,
        config.footprint_lib(),
    )?;
    Ok(())
}

fn ensure_table(
    table_path: &Path,
    kind: TableKind,
    project_root: &Path,
    lib_path: &Path,
) -> Result<(), TableError> {
    let lib_name = lib_name_from_path(kind, lib_path)?;
    let uri = make_uri(lib_path, project_root);

    let mut table = if table_path.exists() {
        let content = fs::read_to_string(table_path)?;
        parse_table(&content, kind)?
    } else {
        default_table(kind)
    };

    ensure_version(&mut table)?;
    ensure_lib_entry(&mut table, &lib_name, &uri);

    let output = table.to_string_pretty_with_indent("  ");
    fs::write(table_path, output)?;
    Ok(())
}

fn parse_table(input: &str, kind: TableKind) -> Result<Sexp, TableError> {
    let sexp = parse_one(input).map_err(|err| TableError::Parse(err.to_string()))?;
    if !matches_root(&sexp, kind.root_name()) {
        return Err(TableError::Invalid(format!(
            "expected root list {}",
            kind.root_name()
        )));
    }
    Ok(sexp)
}

fn default_table(kind: TableKind) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom(Atom::new(kind.root_name())),
        Sexp::List(vec![
            Sexp::Atom(Atom::new("version")),
            Sexp::Atom(Atom::new("7")),
        ]),
    ])
}

fn ensure_version(table: &mut Sexp) -> Result<(), TableError> {
    let items = list_items_mut(table)?;
    for item in items.iter_mut().skip(1) {
        if let Ok(list) = list_items_mut(item) {
            if list.len() >= 2 && atom_value(&list[0]) == Some("version") {
                return Ok(());
            }
        }
    }
    items.insert(
        1,
        Sexp::List(vec![
            Sexp::Atom(Atom::new("version")),
            Sexp::Atom(Atom::new("7")),
        ]),
    );
    Ok(())
}

fn ensure_lib_entry(table: &mut Sexp, name: &str, uri: &str) {
    let items = match list_items_mut(table) {
        Ok(items) => items,
        Err(_) => return,
    };
    for item in items.iter_mut() {
        if lib_name(item) == Some(name) {
            update_lib(item, name, uri);
            return;
        }
    }
    items.push(build_lib_entry(name, uri));
}

fn build_lib_entry(name: &str, uri: &str) -> Sexp {
    Sexp::List(vec![
        Sexp::Atom(Atom::new("lib")),
        Sexp::List(vec![
            Sexp::Atom(Atom::new("name")),
            Sexp::Atom(Atom::new_quoted(name)),
        ]),
        Sexp::List(vec![
            Sexp::Atom(Atom::new("type")),
            Sexp::Atom(Atom::new_quoted("KiCad")),
        ]),
        Sexp::List(vec![
            Sexp::Atom(Atom::new("uri")),
            Sexp::Atom(Atom::new_quoted(uri)),
        ]),
        Sexp::List(vec![
            Sexp::Atom(Atom::new("options")),
            Sexp::Atom(Atom::new_quoted("")),
        ]),
        Sexp::List(vec![
            Sexp::Atom(Atom::new("descr")),
            Sexp::Atom(Atom::new_quoted("")),
        ]),
    ])
}

fn update_lib(sexp: &mut Sexp, name: &str, uri: &str) {
    let items = match list_items_mut(sexp) {
        Ok(items) => items,
        Err(_) => return,
    };
    set_child_value(items, "name", name);
    set_child_value(items, "type", "KiCad");
    set_child_value(items, "uri", uri);
    set_child_value(items, "options", "");
    set_child_value(items, "descr", "");
}

fn set_child_value(items: &mut Vec<Sexp>, key: &str, value: &str) {
    for item in items.iter_mut().skip(1) {
        let list = match item {
            Sexp::List(list) => list,
            _ => continue,
        };
        if list.len() >= 2 && atom_value(&list[0]) == Some(key) {
            list[1] = Sexp::Atom(Atom::new_quoted(value));
            return;
        }
    }
    items.push(Sexp::List(vec![
        Sexp::Atom(Atom::new(key)),
        Sexp::Atom(Atom::new_quoted(value)),
    ]));
}

fn lib_name(sexp: &Sexp) -> Option<&str> {
    let items = match sexp {
        Sexp::List(items) => items,
        _ => return None,
    };
    if atom_value(&items[0]) != Some("lib") {
        return None;
    }
    for item in items.iter().skip(1) {
        if let Sexp::List(list) = item {
            if list.len() >= 2 && atom_value(&list[0]) == Some("name") {
                return atom_value(&list[1]);
            }
        }
    }
    None
}

fn matches_root(sexp: &Sexp, root: &str) -> bool {
    let items = match sexp {
        Sexp::List(items) => items,
        _ => return false,
    };
    if items.is_empty() {
        return false;
    }
    atom_value(&items[0]) == Some(root)
}

fn list_items_mut(sexp: &mut Sexp) -> Result<&mut Vec<Sexp>, TableError> {
    match sexp {
        Sexp::List(items) => Ok(items),
        _ => Err(TableError::Invalid("expected list".to_string())),
    }
}

fn atom_value(sexp: &Sexp) -> Option<&str> {
    match sexp {
        Sexp::Atom(atom) => Some(atom.value()),
        _ => None,
    }
}

fn lib_name_from_path(kind: TableKind, path: &Path) -> Result<String, TableError> {
    let name = match kind {
        TableKind::Symbol => path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| TableError::Invalid("invalid symbol lib path".to_string()))?
            .to_string(),
        TableKind::Footprint => {
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| TableError::Invalid("invalid footprint lib path".to_string()))?;
            if let Some(stripped) = file_name.strip_suffix(".pretty") {
                stripped.to_string()
            } else {
                file_name.to_string()
            }
        }
    };
    Ok(name)
}

fn make_uri(path: &Path, project_root: &Path) -> String {
    let relative = if path.is_absolute() {
        path.strip_prefix(project_root).ok()
    } else {
        Some(path)
    };
    if let Some(rel) = relative {
        format!(
            "${{KIPRJMOD}}/{}",
            rel.to_string_lossy().trim_start_matches("./")
        )
    } else {
        path.to_string_lossy().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn creates_tables_with_entries() {
        let dir = tempdir().unwrap();
        let config = ImportConfig::new(
            PathBuf::from("project_symbols.kicad_sym"),
            PathBuf::from("project_footprints.pretty"),
            PathBuf::from("project_3d"),
        );
        ensure_project_tables(dir.path(), &config).unwrap();
        let sym = fs::read_to_string(dir.path().join("sym-lib-table")).unwrap();
        let fp = fs::read_to_string(dir.path().join("fp-lib-table")).unwrap();
        assert!(sym.contains("sym_lib_table"));
        assert!(sym.contains("project_symbols"));
        assert!(sym.contains("${KIPRJMOD}/project_symbols.kicad_sym"));
        assert!(fp.contains("fp_lib_table"));
        assert!(fp.contains("project_footprints"));
        assert!(fp.contains("${KIPRJMOD}/project_footprints.pretty"));
    }

    #[test]
    fn updates_existing_entry() {
        let dir = tempdir().unwrap();
        let table_path = dir.path().join("sym-lib-table");
        fs::write(
            &table_path,
            "(sym_lib_table (version 7) (lib (name \"project_symbols\")(type \"KiCad\")(uri \"${KIPRJMOD}/old.kicad_sym\")(options \"\")(descr \"\")))",
        )
        .unwrap();
        let config = ImportConfig::new(
            PathBuf::from("project_symbols.kicad_sym"),
            PathBuf::from("project_footprints.pretty"),
            PathBuf::from("project_3d"),
        );
        ensure_project_tables(dir.path(), &config).unwrap();
        let sym = fs::read_to_string(table_path).unwrap();
        assert!(sym.contains("${KIPRJMOD}/project_symbols.kicad_sym"));
    }
}

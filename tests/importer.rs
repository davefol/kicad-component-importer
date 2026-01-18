use kicad_component_importer::importer::{import_source, ImportConfig, ImportError};
use kicad_component_importer::kicad_sym::{AddPolicy, KicadSymbolLib};
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;
use zip::write::FileOptions;
use zip::ZipWriter;

fn write_symbol_lib(path: &Path, symbol_name: &str, footprint_value: &str) {
    let content = format!(
        "(kicad_symbol_lib (version 20231120) (symbol \"{}\" (property \"Footprint\" \"{}\")))",
        symbol_name, footprint_value
    );
    fs::write(path, content).unwrap();
}

fn write_footprint(path: &Path, footprint_name: &str) {
    let content = format!("(footprint \"{}\")", footprint_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn read_symbol_footprint(path: &Path) -> String {
    let content = fs::read_to_string(path).unwrap();
    let lib = KicadSymbolLib::parse(&content).unwrap();
    let symbols = lib.symbols().unwrap();
    symbols.first().unwrap().property_value("Footprint").unwrap()
}

#[test]
fn import_dir_associates_and_copies() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    let source_sym = source.join("lib.kicad_sym");
    write_symbol_lib(&source_sym, "PartA", "");
    let source_fp = source.join("Footprints.pretty").join("MyFootprint.kicad_mod");
    write_footprint(&source_fp, "MyFootprint");

    let dest_sym = temp.path().join("dest.kicad_sym");
    let dest_fp = temp.path().join("Dest.pretty");
    let dest_steps = temp.path().join("steps");
    let config = ImportConfig::new(dest_sym.clone(), dest_fp.clone(), dest_steps);

    let report = import_source(&source, &config, AddPolicy::ReplaceExisting).unwrap();
    assert_eq!(report.symbols_added(), 1);
    assert_eq!(report.footprints_added(), 1);
    assert_eq!(report.step_files_added(), 0);

    let footprint_value = read_symbol_footprint(&dest_sym);
    assert_eq!(footprint_value, "Dest:MyFootprint");
    assert!(dest_fp.join("MyFootprint.kicad_mod").exists());
}

#[test]
fn import_zip_updates_library_prefix() {
    let temp = tempdir().unwrap();
    let zip_path = temp.path().join("source.zip");
    let file = fs::File::create(&zip_path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default();
    zip.start_file("Symbols/lib.kicad_sym", options).unwrap();
    zip.write_all(
        b"(kicad_symbol_lib (version 20231120) (symbol \"PartA\" (property \"Footprint\" \"Old:MyFootprint\")))",
    )
    .unwrap();
    zip.start_file("Footprints.pretty/MyFootprint.kicad_mod", options)
        .unwrap();
    zip.write_all(b"(footprint \"MyFootprint\")").unwrap();
    zip.finish().unwrap();

    let dest_sym = temp.path().join("dest.kicad_sym");
    let dest_fp = temp.path().join("Dest.pretty");
    let dest_steps = temp.path().join("steps");
    let config = ImportConfig::new(dest_sym.clone(), dest_fp.clone(), dest_steps);
    import_source(&zip_path, &config, AddPolicy::ReplaceExisting).unwrap();

    let footprint_value = read_symbol_footprint(&dest_sym);
    assert_eq!(footprint_value, "Dest:MyFootprint");
}

#[test]
fn import_errors_on_ambiguous_footprints() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    let source_sym = source.join("lib.kicad_sym");
    write_symbol_lib(&source_sym, "PartA", "");
    let source_fp_a = source.join("Footprints.pretty").join("A.kicad_mod");
    let source_fp_b = source.join("Footprints.pretty").join("B.kicad_mod");
    write_footprint(&source_fp_a, "A");
    write_footprint(&source_fp_b, "B");

    let dest_sym = temp.path().join("dest.kicad_sym");
    let dest_fp = temp.path().join("Dest.pretty");
    let dest_steps = temp.path().join("steps");
    let config = ImportConfig::new(dest_sym, dest_fp, dest_steps);

    let err = import_source(&source, &config, AddPolicy::ReplaceExisting).unwrap_err();
    match err {
        ImportError::Association(_) => {}
        other => panic!("unexpected error: {:?}", other),
    }
}

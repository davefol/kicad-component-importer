use clap::Parser;
use kicad_component_importer::cli::{Cli, Command};

#[test]
fn parse_import_command() {
    let cli = Cli::try_parse_from([
        "kci",
        "import",
        "source.zip",
        "--symbol-lib",
        "sym.kicad_sym",
        "--footprint-lib",
        "foot.pretty",
        "--step-dir",
        "steps",
    ])
    .unwrap();
    match cli.command {
        Command::Import(args) => {
            assert_eq!(args.source.to_string_lossy(), "source.zip");
            assert_eq!(
                args.symbol_lib.unwrap().to_string_lossy(),
                "sym.kicad_sym"
            );
            assert_eq!(
                args.footprint_lib.unwrap().to_string_lossy(),
                "foot.pretty"
            );
            assert_eq!(args.step_dir.unwrap().to_string_lossy(), "steps");
        }
    }
}

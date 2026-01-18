use clap::Parser;

fn main() {
    let cli = kicad_component_importer::cli::Cli::parse();
    if let Err(err) = kicad_component_importer::cli::run(cli) {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

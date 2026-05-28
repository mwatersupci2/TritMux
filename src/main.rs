use std::env;
use std::path::PathBuf;
use std::process;

fn main() {
    let mut tui_mode = false;
    let mut explicit_store: Option<PathBuf> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tui" | "tui" => tui_mode = true,
            "--store" => {
                let Some(path) = args.next() else {
                    eprintln!("tritmux: --store needs a path");
                    process::exit(2);
                };
                explicit_store = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => {
                eprintln!("tritmux: unknown argument '{other}'");
                print_help();
                process::exit(2);
            }
        }
    }

    let path = explicit_store
        .or_else(|| env::var_os("TRITMUX_STORE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("tritmux-notes.trit"));

    let result = if tui_mode {
        let mut store = match tritmux::notes::NoteStore::load(&path) {
            Ok(store) => store,
            Err(err) => {
                eprintln!("tritmux: {err}");
                process::exit(1);
            }
        };
        tritmux::tui::run(&mut store, &path).map_err(|err| err.to_string())
    } else {
        tritmux::ui::run(path).map_err(|err| err.to_string())
    };

    if let Err(err) = result {
        eprintln!("tritmux: {err}");
        process::exit(1);
    }
}

fn print_help() {
    println!("usage: tritmux [--tui] [--store PATH]");
    println!("  --tui        open the split editor/observability frontend");
    println!("  --store PATH use PATH as the trinary note artifact");
}

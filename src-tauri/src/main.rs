#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "probe") {
        std::process::exit(iran_split_desktop_lib::run_probe_cli(&args[1..]));
    }
    iran_split_desktop_lib::run();
}

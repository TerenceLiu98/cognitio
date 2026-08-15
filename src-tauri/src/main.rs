// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match llmwiki_lib::run_cli(&args) {
        Ok(true) => return,
        Err(error) => {
            eprintln!("llmwiki: {error}");
            std::process::exit(1);
        }
        Ok(false) => {}
    }
    llmwiki_lib::run()
}

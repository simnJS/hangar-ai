// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Same executable, two roles: `--mcp` turns it into a stdio MCP server for
    // agent CLIs. Shipping one binary means agent configs only ever need this
    // path, with no sidecar to bundle or locate.
    if std::env::args().any(|arg| arg == "--mcp") {
        hangar_ai_lib::run_mcp();
        return;
    }
    hangar_ai_lib::run()
}

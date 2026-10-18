use lobbylocker_core::{
    connections,
    firewall::{self, Request},
    latency,
    model::resolve_rules,
    store::Store,
};
use std::path::PathBuf;

fn main() {
    if let Some(code) = firewall::helper_entry() {
        std::process::exit(code);
    }
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> lobbylocker_core::Result<()> {
    let command = std::env::args().nth(1).unwrap_or_else(|| "help".into());
    if command == "reset" {
        return firewall::elevate(&Request::Reset {});
    }
    let defaults = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../data/games");
    let store = Store::open(Store::default_directory()?, &defaults)?;
    let snapshot = store.snapshot()?;
    match command.as_str() {
        "list" => println!("{}", serde_json::to_string_pretty(&snapshot).map_err(|e| e.to_string())?),
        "preview" => {
            let rules = resolve_rules(&snapshot.games, &snapshot.preferences)?;
            let request = Request::Apply { rules };
            let script = if cfg!(target_os = "windows") { firewall::windows_preview(&request)? } else { firewall::nft_script(&request, false)? };
            println!("{script}");
        }
        "apply" => firewall::elevate(&Request::Apply { rules: resolve_rules(&snapshot.games, &snapshot.preferences)? })?,
        "ping" => println!("{}", serde_json::to_string_pretty(&latency::measure(&snapshot.games)).map_err(|e| e.to_string())?),
        "connections" => println!("{}", serde_json::to_string_pretty(&connections::scan()?).map_err(|e| e.to_string())?),
        _ => println!("LobbyLocker CLI\nCommands: list, preview, apply, reset, ping, connections\nFirewall changes request administrator access. preview never changes the firewall."),
    }
    Ok(())
}

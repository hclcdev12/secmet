//! Binaire privilégié SecMet : `secmet-helper serve [--socket-dir DIR] [--allow-uid UID]`.
//!
//! Lancé par l'UI via `pkexec` (un seul prompt Polkit) ; voir [`secmet_helper::server`].

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rest: &[String] = match args.get(1).map(String::as_str) {
        Some("serve") => &args[2..],
        Some("--help") | Some("-h") => {
            eprintln!("usage: secmet-helper serve [--socket-dir DIR] [--allow-uid UID] [--vault-dir DIR] [--ttl SECONDS]");
            std::process::exit(2);
        }
        _ => {
            eprintln!("usage: secmet-helper serve [--socket-dir DIR] [--allow-uid UID] [--vault-dir DIR] [--ttl SECONDS]");
            std::process::exit(2);
        }
    };
    std::process::exit(secmet_helper::server::main(rest));
}

//! `layoutcraft-mcp --root <folder>`: one JSON-RPC 2.0 message per line on stdin, the answers on stdout (docs/35-mcp-native.md).
use std::io::{BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = args.iter().position(|a| a == "--root").and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| ".".to_string());
    let mut server = layoutcraft_mcp::Server::new(std::path::PathBuf::from(root));
    let stdout = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let mut out = stdout.lock();
        for answer in server.handle_line(&line) {
            let _ = writeln!(out, "{answer}");
        }
        let _ = out.flush();
    }
}

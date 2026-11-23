//! Help > AI assistant (MCP): the text a user puts into an MCP client so that it starts the LayoutCraft MCP server. A port of `web/mcpinfo.js`
//! (the web test cases are repeated below). The server is a separate program that the client starts and talks to over stdio.

/// The configuration for a client: `json` is the `mcpServers` block for a client that reads a JSON configuration (Claude Desktop and others),
/// `claude_code` the one-line command for Claude Code (`claude mcp add`), `args` the full argument list including `--root`.
#[derive(Debug, Clone, PartialEq)]
pub struct McpConfig {
    pub json: String,
    pub claude_code: String,
    pub command: String,
    pub args: Vec<String>,
}

fn is_plain(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "_./:@%+=,-".contains(c))
}

/// A word of the shell command: as is when it only holds safe characters, else in double quotes with `"`, `$` and the backtick escaped.
fn quote(text: &str) -> String {
    if is_plain(text) {
        return text.to_string();
    }
    let mut out = String::from("\"");
    for c in text.chars() {
        if matches!(c, '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

/// `command` is the program (default "node"), `args` the arguments before `--root` (`None`: just `server`), `root` the folder the server may read
/// and write, `name` the name of the server in the client.
pub fn mcp_config(command: &str, args: Option<&[String]>, server: &str, root: &str, name: &str) -> McpConfig {
    let mut full: Vec<String> = match args {
        Some(head) => head.to_vec(),
        None => vec![server.to_string()],
    };
    full.push("--root".to_string());
    full.push(root.to_string());
    let json_args = full.iter().map(|arg| format!("        {}", json_string(arg))).collect::<Vec<_>>().join(",\n");
    let json = format!(
        "{{\n  \"mcpServers\": {{\n    {}: {{\n      \"command\": {},\n      \"args\": [\n{}\n      ]\n    }}\n  }}\n}}",
        json_string(name),
        json_string(command),
        json_args
    );
    let words: Vec<String> = std::iter::once(command.to_string()).chain(full.iter().cloned()).map(|word| quote(&word)).collect();
    McpConfig { json, claude_code: format!("claude mcp add {name} -- {}", words.join(" ")), command: command.to_string(), args: full }
}

/// The native MCP server (`layoutcraft-mcp`) when it sits next to the running app: the app and the server ship in one folder.
pub fn bundled_server() -> Option<std::path::PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let name = if cfg!(windows) { "layoutcraft-mcp.exe" } else { "layoutcraft-mcp" };
    let path = dir.join(name);
    path.is_file().then_some(path)
}

/// The configuration that starts the native server `exe` (no node needed): its command is the program itself, its arguments just `--root <folder>`.
pub fn native_config(exe: &str, root: &str, name: &str) -> McpConfig {
    mcp_config(exe, Some(&[]), "", root, name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_native_server_needs_no_node_and_only_a_root() {
        let config = native_config(r"C:\Apps\LayoutCraft\layoutcraft-mcp.exe", r"D:\Drawings", "layoutcraft");
        let entry = parsed(&config, "layoutcraft");
        assert_eq!(entry["command"], r"C:\Apps\LayoutCraft\layoutcraft-mcp.exe");
        let args: Vec<&str> = entry["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        assert_eq!(args, ["--root", r"D:\Drawings"]);
        assert!(config.claude_code.starts_with("claude mcp add layoutcraft -- "));
        assert!(!config.claude_code.contains("node"));
    }

    use super::*;

    fn parsed(config: &McpConfig, name: &str) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(&config.json).unwrap()["mcpServers"][name].clone()
    }

    #[test]
    fn node_server_root_and_a_folder_with_a_space_is_quoted() {
        let config = mcp_config("node", None, "/opt/layoutcraft/mcp/server.mjs", "/home/me/My Drawings", "layoutcraft");
        let entry = parsed(&config, "layoutcraft");
        assert_eq!(entry["command"], "node");
        let args: Vec<&str> = entry["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        assert_eq!(args.join("|"), "/opt/layoutcraft/mcp/server.mjs|--root|/home/me/My Drawings");
        assert_eq!(config.claude_code, "claude mcp add layoutcraft -- node /opt/layoutcraft/mcp/server.mjs --root \"/home/me/My Drawings\"");
    }

    #[test]
    fn a_windows_path_survives_the_json_round_trip_and_is_quoted_in_the_command() {
        let server = "C:\\Program Files\\LayoutCraft\\mcp\\server.mjs";
        let config = mcp_config("node", None, server, "D:\\Drawings", "layoutcraft");
        let entry = parsed(&config, "layoutcraft");
        let args: Vec<&str> = entry["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        assert_eq!(args.join("|"), format!("{server}|--root|D:\\Drawings"));
        assert_eq!(config.claude_code, format!("claude mcp add layoutcraft -- node \"{server}\" --root \"D:\\Drawings\""));
    }

    #[test]
    fn a_bundled_program_has_no_node_and_no_server_file_and_the_name_is_free() {
        let config = mcp_config("/opt/lc/layoutcraft-mcp", Some(&[]), "", "/x", "lc");
        let entry = parsed(&config, "lc");
        assert_eq!(entry["command"], "/opt/lc/layoutcraft-mcp");
        assert_eq!(config.args.join("|"), "--root|/x");
        assert_eq!(config.claude_code, "claude mcp add lc -- /opt/lc/layoutcraft-mcp --root /x");
    }

    #[test]
    fn quotes_dollars_and_backticks_are_escaped_and_the_json_is_the_web_layout() {
        assert_eq!(quote("a b\"c$d`e"), "\"a b\\\"c\\$d\\`e\"");
        assert_eq!(quote("plain/path-1.2"), "plain/path-1.2");
        let config = mcp_config("node", None, "s.mjs", "r", "layoutcraft");
        assert_eq!(config.json, "{\n  \"mcpServers\": {\n    \"layoutcraft\": {\n      \"command\": \"node\",\n      \"args\": [\n        \"s.mjs\",\n        \"--root\",\n        \"r\"\n      ]\n    }\n  }\n}");
    }
}

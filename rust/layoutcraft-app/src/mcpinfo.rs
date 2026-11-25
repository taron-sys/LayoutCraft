//! Help > AI assistant (MCP): the text a user puts into an MCP client so that it starts the LayoutCraft MCP server (`layoutcraft-mcp`, see
//! docs/32-mcp.md). The server is a separate program that the client starts and talks to over stdio; its only argument is `--root <folder>`.
//! The same rules are in `web/mcpinfo.js` for the browser shell (the test cases below are shared with it).

/// The configuration for a client: `json` is the `mcpServers` block for a client that reads a JSON configuration (Claude Desktop and others),
/// `claude_code` the one-line command for Claude Code (`claude mcp add`), `args` the full argument list (`--root` and the folder).
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

/// The configuration that starts the server program `command` with `--root <root>`; `name` is the name of the server in the client.
pub fn mcp_config(command: &str, root: &str, name: &str) -> McpConfig {
    let args = vec!["--root".to_string(), root.to_string()];
    let json_args = args.iter().map(|arg| format!("        {}", json_string(arg))).collect::<Vec<_>>().join(",\n");
    let json = format!(
        "{{\n  \"mcpServers\": {{\n    {}: {{\n      \"command\": {},\n      \"args\": [\n{}\n      ]\n    }}\n  }}\n}}",
        json_string(name),
        json_string(command),
        json_args
    );
    let words: Vec<String> = std::iter::once(command).chain(args.iter().map(String::as_str)).map(quote).collect();
    McpConfig { json, claude_code: format!("claude mcp add {name} -- {}", words.join(" ")), command: command.to_string(), args }
}

/// The file name of the server program on this platform.
pub fn server_file_name() -> &'static str {
    if cfg!(windows) { "layoutcraft-mcp.exe" } else { "layoutcraft-mcp" }
}

/// What the server field shows before the user types a path (the server was not found next to the app).
pub fn server_placeholder() -> &'static str {
    if cfg!(windows) { r"C:\path\to\LayoutCraft\layoutcraft-mcp.exe" } else { "/path/to/LayoutCraft/layoutcraft-mcp" }
}

/// The server program when it sits next to the running app: the app and the server ship in one folder.
pub fn bundled_server() -> Option<std::path::PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let path = dir.join(server_file_name());
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(config: &McpConfig, name: &str) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(&config.json).unwrap()["mcpServers"][name].clone()
    }

    fn args(entry: &serde_json::Value) -> Vec<String> {
        entry["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_server_gets_only_a_root_and_needs_no_other_program() {
        let config = mcp_config(r"C:\Apps\LayoutCraft\layoutcraft-mcp.exe", r"D:\Drawings", "layoutcraft");
        let entry = parsed(&config, "layoutcraft");
        assert_eq!(entry["command"], r"C:\Apps\LayoutCraft\layoutcraft-mcp.exe");
        assert_eq!(args(&entry), ["--root", r"D:\Drawings"]);
        assert!(config.claude_code.starts_with("claude mcp add layoutcraft -- "));
        assert!(!config.claude_code.contains("node"));
    }

    #[test]
    fn a_folder_with_a_space_is_quoted() {
        let config = mcp_config("/opt/layoutcraft/layoutcraft-mcp", "/home/me/My Drawings", "layoutcraft");
        assert_eq!(args(&parsed(&config, "layoutcraft")).join("|"), "--root|/home/me/My Drawings");
        assert_eq!(config.claude_code, "claude mcp add layoutcraft -- /opt/layoutcraft/layoutcraft-mcp --root \"/home/me/My Drawings\"");
    }

    #[test]
    fn a_windows_path_survives_the_json_round_trip_and_is_quoted_in_the_command() {
        let server = "C:\\Program Files\\LayoutCraft\\layoutcraft-mcp.exe";
        let config = mcp_config(server, "D:\\Drawings", "layoutcraft");
        let entry = parsed(&config, "layoutcraft");
        assert_eq!(entry["command"], server);
        assert_eq!(args(&entry).join("|"), "--root|D:\\Drawings");
        assert_eq!(config.claude_code, format!("claude mcp add layoutcraft -- \"{server}\" --root \"D:\\Drawings\""));
    }

    #[test]
    fn the_server_name_is_free() {
        let config = mcp_config("/opt/lc/layoutcraft-mcp", "/x", "lc");
        assert_eq!(parsed(&config, "lc")["command"], "/opt/lc/layoutcraft-mcp");
        assert_eq!(config.args.join("|"), "--root|/x");
        assert_eq!(config.claude_code, "claude mcp add lc -- /opt/lc/layoutcraft-mcp --root /x");
    }

    #[test]
    fn quotes_dollars_and_backticks_are_escaped_and_the_json_is_the_web_layout() {
        assert_eq!(quote("a b\"c$d`e"), "\"a b\\\"c\\$d\\`e\"");
        assert_eq!(quote("plain/path-1.2"), "plain/path-1.2");
        assert_eq!(quote(""), "\"\"");
        let config = mcp_config("srv", "r", "layoutcraft");
        assert_eq!(config.json, "{\n  \"mcpServers\": {\n    \"layoutcraft\": {\n      \"command\": \"srv\",\n      \"args\": [\n        \"--root\",\n        \"r\"\n      ]\n    }\n  }\n}");
    }

    #[test]
    fn the_placeholder_names_the_program_of_this_platform() {
        assert!(server_placeholder().ends_with(server_file_name()));
    }
}

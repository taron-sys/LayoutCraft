// Help > AI assistant (MCP): the text a user puts into an MCP client so that it starts the LayoutCraft MCP server (mcp/server.mjs, see docs/32-mcp.md). Pure functions, no DOM.
// The server is a separate program that the client starts and talks to over stdio; the web page cannot start it. The desktop build is the native app (docs/34-desktop-build.md): it has its own window with the same text. The hook below is for a host that embeds this page and ships the server
// as a program: window.layoutcraftDesktop.mcp = { command, args } (args = the arguments before --root); no such host is planned.

const quote = (t) => (/^[A-Za-z0-9_./:@%+=,-]+$/.test(t) ? t : `"${String(t).replace(/(["$`])/g, (m) => String.fromCharCode(92) + m)}"`);

/**
 * The configuration for a client. Options: `command` (default "node"), `args` (the arguments before --root; default [server]), `server` (path of mcp/server.mjs, used when no args are given),
 * `root` (the folder the server may read and write), `name` (the name of the server in the client, default "layoutcraft").
 * Returns { json, claudeCode, command, args }: `json` is the `mcpServers` block for a client that reads a JSON configuration (Claude Desktop and others), `claudeCode` the one-line
 * command for Claude Code (`claude mcp add`), `args` the full argument list including --root.
 */
export function mcpConfig({ command = 'node', args = null, server = '', root = '', name = 'layoutcraft' } = {}) {
  const head = Array.isArray(args) ? args.map(String) : [String(server)];
  const full = [...head, '--root', String(root)];
  return {
    json: JSON.stringify({ mcpServers: { [name]: { command, args: full } } }, null, 2),
    claudeCode: `claude mcp add ${name} -- ${[command, ...full].map(quote).join(' ')}`,
    command,
    args: full,
  };
}

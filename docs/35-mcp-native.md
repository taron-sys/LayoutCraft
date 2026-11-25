# 35 - native MCP server (N5)

Crate `rust/layoutcraft-mcp` (bin `layoutcraft-mcp --root <folder>`): stdio JSON-RPC 2.0, one message per line, same protocol, resources and tools as the Node server `mcp/server.mjs`, which was the reference while this crate was written and is removed now ([42](42-javascript-removal.md)). `tools/list` is served from `tests/oracle/tools-list.json`: it was dumped from the Node server and is now the file of record, edited together with the tool it describes; a Rust test compares it with the list the server answers.

Parity run (the maintainers' harness, private repository): `LAYOUTCRAFT_MCP_BIN=<exe> node visio-test-harness/test-mcp.mjs` (107 checks) against the built program; the mode of that script that started the Node server has nothing left to start.

## Progress
- Step 398: protocol, resources, tools/list, 7 tools.
- Step 399: 32 more tools (66/107 checks).
- Step 400: the last 10 tools (stencils, picture, check_diagram, compare_files, inventory, find_shapes, borders, get_page). **Rust server: 107/107 checks of test-mcp.mjs; node server 107/107.** Rust unit tests: 4 (tools-list identical to node, every listed tool implemented, draw/list/root confinement, path rule).

## Tool table (49 of 49 done)
| tool | status |
|---|---|
| new_drawing | done |
| open | done |
| save | done |
| export | done |
| list_pages | done |
| list_shapes | done |
| draw_shape | done |
| add_connector | done |
| set_text | done |
| style_shapes | done |
| delete_shapes | done |
| add_page | done |
| move_shapes | done |
| rotate_shapes | done |
| flip_shapes | done |
| align_shapes | done |
| distribute_shapes | done |
| group_shapes | done |
| ungroup_shape | done |
| reorder_shapes | done |
| list_connectors | done |
| set_shape_name | done |
| set_alt_text | done |
| set_hyperlinks | done |
| set_shape_data | done |
| set_page_size | done |
| rename_page | done |
| duplicate_page | done |
| delete_page | done |
| add_layer | done |
| list_layers | done |
| set_layer | done |
| assign_layer | done |
| delete_layer | done |
| shape_operation | done |
| check_diagram | done |
| compare_files | done |
| inventory | done |
| find_shapes | done |
| add_border | done |
| remove_border | done |
| get_page | done |
| insert_picture | done |
| open_stencil | done |
| drop_master | done |
| list_themes | done |
| apply_theme | done |
| undo | done |
| redo | done |

//! The MCP server on the native engine (docs/32-mcp.md, docs/35-mcp-native.md). `tests/oracle/tools-list.json` is the tool list of the server:
//! it is edited here, together with the tool it describes, and served as is. The retired Node server (last in commit c9aa852) had the same list.
use std::path::{Component, Path, PathBuf};

use layoutcraft_engine::Engine;
use serde_json::{json, Value};

const TOOLS_LIST: &str = include_str!("../tests/oracle/tools-list.json");
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const INSTRUCTIONS: &str = "Coordinates are inches, the origin is the lower left corner of the page, y grows upward. Draw shapes with draw_shape, join them with add_connector, then save as .vsdx or export.";
const PX_PER_INCH: f64 = 96.0;

pub struct Server {
    base: PathBuf,
    engine: Engine,
}

type ToolResult = Result<String, String>;

fn text_result(r: ToolResult) -> Value {
    match r {
        Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
        Err(text) => json!({ "isError": true, "content": [{ "type": "text", "text": text }] }),
    }
}

/// JSON.stringify(v, null, 1)
fn pretty(v: &Value) -> String {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, formatter);
    serde::Serialize::serialize(v, &mut ser).expect("serialise");
    String::from_utf8(out).expect("utf8")
}

/// String(v) for a JSON value: strings as they are, numbers and the rest as JSON.
fn id_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn ids_of(a: &Value) -> Result<Vec<String>, String> {
    match a.get("ids").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => Ok(list.iter().map(id_text).collect()),
        _ => Err("ids must be a list of shape ids".into()),
    }
}

fn num(a: &Value, k: &str) -> Result<f64, String> {
    a.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| format!("{k} must be a number"))
}

/// JS truthiness of a JSON value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// String(v ?? '').toLowerCase()
fn js_lower(v: &Value) -> String {
    if v.is_null() { String::new() } else { id_text(v).to_lowercase() }
}

fn flag(a: &Value, k: &str) -> i32 {
    a.get(k).is_some_and(truthy) as i32
}

/// A number the way JS prints it inside a template string (`${w}`): 11 not 11.0.
fn js_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn inch(v: f64) -> f64 {
    (v / PX_PER_INCH * 1e6).round() / 1e6
}

impl Server {
    pub fn new(root: PathBuf) -> Self {
        let mut engine = Engine::new();
        engine.new_blank(11.0, 8.5, false, "Page-1").expect("blank drawing");
        Self { base: root, engine }
    }

    /// A path below the root folder (anything else is refused).
    fn inside(&self, p: Option<&Value>) -> Result<PathBuf, String> {
        let text = p.and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("a path is required")?;
        let mut full = self.base.clone();
        let mut depth = 0usize;
        for part in Path::new(text).components() {
            match part {
                Component::Normal(n) => {
                    full.push(n);
                    depth += 1;
                }
                Component::CurDir => {}
                Component::ParentDir if depth > 0 => {
                    full.pop();
                    depth -= 1;
                }
                _ => return Err(format!("the path is outside the root folder: {text}")),
            }
        }
        Ok(full)
    }

    fn page(&self, a: &Value) -> Result<usize, String> {
        let count = self.engine.page_count().unwrap_or(0);
        let n = match a.get("page") {
            None | Some(Value::Null) => 0.0,
            Some(v) => v.as_f64().unwrap_or(f64::NAN),
        };
        if n.fract() != 0.0 || n < 0.0 || n >= count as f64 {
            return Err(format!("no page {} (the drawing has {count})", a.get("page").map_or("0".to_string(), |v| v.to_string().trim_matches('"').to_string())));
        }
        Ok(n as usize)
    }

    fn model(&self) -> Value {
        self.engine.json().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
    }

    pub fn handle_line(&mut self, line: &str) -> Vec<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return vec![json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } }).to_string()],
        };
        let batch = match msg {
            Value::Array(list) => list,
            one => vec![one],
        };
        batch.iter().filter_map(|one| self.handle(one)).map(|v| v.to_string()).collect()
    }

    /// One JSON-RPC message in, the response (None for a notification) out.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str);
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let ok = |result: Value| Some(json!({ "jsonrpc": "2.0", "id": id.clone(), "result": result }));
        let err = |code: i64, message: String| Some(json!({ "jsonrpc": "2.0", "id": id.clone().unwrap_or(Value::Null), "error": { "code": code, "message": message } }));
        let Some(method) = method else {
            return if id.is_none() { None } else { err(-32600, "invalid request".into()) };
        };
        id.as_ref()?; // notifications need no answer
        match method {
            "initialize" => {
                let want = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = if PROTOCOLS.contains(&want) { want } else { PROTOCOLS[0] };
                ok(json!({ "protocolVersion": version, "capabilities": { "tools": { "listChanged": false }, "resources": { "listChanged": false } }, "serverInfo": { "name": "layoutcraft", "version": "0.1.0" }, "instructions": INSTRUCTIONS }))
            }
            "ping" => ok(json!({})),
            "tools/list" => ok(serde_json::from_str(TOOLS_LIST).expect("tools-list.json")),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else { return err(-32602, "name is required".into()) };
                let args = match params.get("arguments") {
                    Some(v) if !v.is_null() => v.clone(),
                    _ => json!({}),
                };
                ok(text_result(self.call_tool(name, &args)))
            }
            "resources/list" => {
                let names: Vec<String> = self.engine.page_names_json().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
                let mut list: Vec<Value> = names.iter().enumerate().map(|(i, n)| json!({ "uri": format!("layoutcraft://page/{i}/svg"), "name": format!("{n} (SVG)"), "mimeType": "image/svg+xml" })).collect();
                list.push(json!({ "uri": "layoutcraft://model", "name": "Drawing model (JSON)", "mimeType": "application/json" }));
                ok(json!({ "resources": list }))
            }
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                if uri == "layoutcraft://model" {
                    return ok(json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": self.engine.json().unwrap_or_default() }] }));
                }
                let page = uri.strip_prefix("layoutcraft://page/").and_then(|r| r.strip_suffix("/svg")).filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())).and_then(|d| d.parse::<usize>().ok());
                if let Some(n) = page.filter(|n| *n < self.engine.page_count().unwrap_or(0)) {
                    if let Some(Ok(svg)) = self.engine.svg(n, false) {
                        return ok(json!({ "contents": [{ "uri": uri, "mimeType": "image/svg+xml", "text": svg }] }));
                    }
                }
                err(-32002, format!("no such resource: {uri}"))
            }
            "prompts/list" => ok(json!({ "prompts": [] })),
            other => err(-32601, format!("method not found: {other}")),
        }
    }

    fn call_tool(&mut self, name: &str, a: &Value) -> ToolResult {
        match name {
            "new_drawing" => {
                let num = |k: &str, d: f64| a.get(k).and_then(Value::as_f64).unwrap_or(d);
                let label = a.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("Page-1");
                self.engine.new_blank(num("width", 11.0), num("height", 8.5), a.get("metric").and_then(Value::as_bool).unwrap_or(false), label)?;
                Ok("new drawing".into())
            }
            "open" => {
                let full = self.inside(a.get("path"))?;
                let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
                let shown = a["path"].as_str().unwrap_or("");
                match full.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                    Some("drawio") => {
                        self.engine.new_blank(11.0, 8.5, false, "Page-1")?;
                        let r = self.engine.import_drawio(&bytes)?;
                        Ok(format!("opened {shown} (draw.io): {r:?}"))
                    }
                    Some("excalidraw") => {
                        self.engine.new_blank(11.0, 8.5, false, "Page-1")?;
                        let r = self.engine.import_excalidraw(&String::from_utf8_lossy(&bytes))?;
                        Ok(format!("opened {shown} (Excalidraw): {r:?}"))
                    }
                    _ => {
                        self.engine.open(bytes)?;
                        Ok(format!("opened {shown}: {} page(s)", self.engine.page_count().unwrap_or(0)))
                    }
                }
            }
            "save" => {
                let full = self.inside(a.get("path"))?;
                if full.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() != Some("vsdx") {
                    return Err("the file name must end in .vsdx".into());
                }
                if let Some(dir) = full.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(&full, self.engine.export().ok_or("no document")?).map_err(|e| e.to_string())?;
                Ok(format!("saved {}", a["path"].as_str().unwrap_or("")))
            }
            "list_pages" => {
                let names: Vec<String> = serde_json::from_str(&self.engine.page_names_json()?).map_err(|e| e.to_string())?;
                let model = self.model();
                let mut rows = Vec::new();
                for (i, n) in names.iter().enumerate() {
                    let size: Value = serde_json::from_str(&self.engine.page_size_json(i)?).map_err(|e| e.to_string())?;
                    let shapes = model["pages"][i]["shapes"].as_array().map_or(0, Vec::len);
                    rows.push(json!({ "index": i, "name": n, "width": size["w"], "height": size["h"], "shapes": shapes }));
                }
                Ok(pretty(&Value::Array(rows)))
            }
            "list_shapes" => {
                let p = self.page(a)?;
                let size: Value = serde_json::from_str(&self.engine.page_size_json(p)?).map_err(|e| e.to_string())?;
                let page_h = size["h"].as_f64().unwrap_or(0.0);
                let names: Value = serde_json::from_str(&self.engine.shape_names_json(p)?).map_err(|e| e.to_string())?;
                let model = self.model();
                let rows: Vec<Value> = model["pages"][p]["shapes"].as_array().map_or_else(Vec::new, |list| list.iter().map(|s| shape_row(s, page_h, &names)).collect());
                Ok(pretty(&Value::Array(rows)))
            }
            "export" => {
                let p = self.page(a)?;
                let format = a.get("format").and_then(Value::as_str).unwrap_or("");
                let text = match format {
                    "svg" => self.engine.svg(p, false).ok_or("no document")??,
                    "dxf" => self.engine.export_dxf(p)?.dxf,
                    "drawio" => self.engine.export_drawio()?,
                    "excalidraw" => pretty(&self.engine.export_excalidraw(p)?),
                    _ => return Err("format must be svg, dxf, drawio or excalidraw".into()),
                };
                let Some(shown) = a.get("path").and_then(Value::as_str).filter(|s| !s.is_empty()) else { return Ok(text) };
                let full = self.inside(Some(&json!(shown)))?;
                if let Some(dir) = full.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(&full, &text).map_err(|e| e.to_string())?;
                Ok(format!("wrote {shown} ({} characters)", text.encode_utf16().count()))
            }
            "draw_shape" => {
                let p = self.page(a)?;
                let kinds = ["rectangle", "rounded", "ellipse", "diamond", "triangle", "textbox", "line"];
                let kind = a.get("kind").and_then(Value::as_str).and_then(|k| kinds.iter().position(|x| *x == k)).ok_or_else(|| format!("kind must be one of {}", kinds.join(", ")))?;
                let (x0, y0, x1, y1) = (num(a, "x0")?, num(a, "y0")?, num(a, "x1")?, num(a, "y1")?);
                let id = self.engine.draw(p, kind as i32, x0, y0, x1, y1)?;
                if let Some(text) = a.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    self.engine.edit(p, 8, &format!("{id}\n{text}"), 0.0, 0.0, 0.0, 0.0, 0)?;
                }
                if let Some(style) = a.get("style").filter(|s| !s.is_null()) {
                    self.engine.edit(p, 10, &format!("{id}\n{style}"), 0.0, 0.0, 0.0, 0.0, 0)?;
                }
                Ok(pretty(&json!({ "id": id })))
            }
            "add_connector" => {
                let p = self.page(a)?;
                let end = |k: &str| -> Result<(String, Option<usize>), String> {
                    let e = a.get(k).ok_or_else(|| format!("{k} is required"))?;
                    let id = e.get("id").map(id_text).ok_or("id is required")?;
                    let side = e.get("side").and_then(Value::as_u64).filter(|s| *s > 0).map(|s| s as usize);
                    Ok((id, side))
                };
                let (from, from_site) = end("from")?;
                let (to, to_site) = end("to")?;
                let id = self.engine.add_connector(p, &from, from_site, &to, to_site)?;
                Ok(pretty(&json!({ "id": id })))
            }
            "set_text" => {
                let p = self.page(a)?;
                let id = a.get("id").map(id_text).unwrap_or_default();
                let text = a.get("text").map(id_text).unwrap_or_default();
                self.engine.edit(p, 8, &format!("{id}\n{text}"), 0.0, 0.0, 0.0, 0.0, 0)?;
                Ok("ok".into())
            }
            "style_shapes" => {
                let p = self.page(a)?;
                let ids = ids_of(a)?;
                let style = a.get("style").filter(|s| !s.is_null()).cloned().unwrap_or_else(|| json!({}));
                self.engine.edit(p, 10, &format!("{}\n{style}", ids.join("\n")), 0.0, 0.0, 0.0, 0.0, 0)?;
                Ok("ok".into())
            }
            "delete_shapes" => {
                let p = self.page(a)?;
                self.engine.edit(p, 6, &ids_of(a)?.join("\n"), 0.0, 0.0, 0.0, 0.0, 0)?;
                Ok("ok".into())
            }
            "add_page" => {
                let count = self.engine.page_count().unwrap_or(0);
                let after = a.get("after").and_then(Value::as_f64).map_or(count.saturating_sub(1), |v| v.max(0.0) as usize);
                self.engine.page_op(1, after, 0, a.get("name").and_then(Value::as_str).unwrap_or(""))?;
                Ok(format!("pages: {}", self.engine.page_count().unwrap_or(0)))
            }
            "move_shapes" => {
                let p = self.page(a)?;
                let ids = ids_of(a)?;
                let (dx, dy) = (num(a, "dx")?, num(a, "dy")?);
                if ids.len() == 1 {
                    self.engine.edit(p, 2, &ids[0], dx, dy, 0.0, 0.0, 0)?;
                } else {
                    self.engine.edit(p, 5, &ids.join("\n"), dx, dy, 0.0, 0.0, 0)?;
                }
                Ok("ok".into())
            }
            "rotate_shapes" => {
                let p = self.page(a)?;
                let ids = ids_of(a)?;
                self.engine.edit(p, 11, &ids.join("\n"), num(a, "degrees")? * std::f64::consts::PI / 180.0, 0.0, 0.0, 0.0, 0)?;
                Ok("ok".into())
            }
            "flip_shapes" => {
                let p = self.page(a)?;
                let ids = ids_of(a)?;
                self.engine.edit(p, 12, &ids.join("\n"), 0.0, 0.0, 0.0, 0.0, flag(a, "vertical"))?;
                Ok("ok".into())
            }
            "align_shapes" => {
                let p = self.page(a)?;
                let modes = ["left", "center", "right", "top", "middle", "bottom"];
                let ids = ids_of(a)?;
                let m = a.get("mode").and_then(Value::as_str).and_then(|m| modes.iter().position(|x| *x == m)).ok_or("mode must be left, center, right, top, middle or bottom")?;
                self.engine.edit(p, 14, &ids.join("\n"), m as f64, 0.0, 0.0, 0.0, 0)?;
                Ok("ok".into())
            }
            "distribute_shapes" => {
                let ids = ids_of(a)?;
                if ids.len() < 3 {
                    return Err("distribute needs at least three shapes".into());
                }
                let p = self.page(a)?;
                self.engine.edit(p, 15, &ids.join("\n"), 0.0, 0.0, 0.0, 0.0, flag(a, "vertical"))?;
                Ok("ok".into())
            }
            "group_shapes" => {
                let ids = ids_of(a)?;
                if ids.len() < 2 {
                    return Err("a group needs at least two shapes".into());
                }
                let p = self.page(a)?;
                let id = self.engine.group_op(p, 0, &ids.join("\n"))?;
                Ok(pretty(&json!({ "id": id })))
            }
            "ungroup_shape" => {
                let p = self.page(a)?;
                self.engine.group_op(p, 1, &a.get("id").map(id_text).unwrap_or_default())?;
                Ok("ok".into())
            }
            "reorder_shapes" => {
                let p = self.page(a)?;
                let ids = ids_of(a)?;
                let how = a.get("how").and_then(Value::as_str).unwrap_or("");
                let op = ["front", "back", "forward", "backward"].iter().position(|x| *x == how).ok_or_else(|| format!("unknown drawing order {how}"))? as i32 + 2;
                self.engine.group_op(p, op, &ids.join("\n"))?;
                Ok("ok".into())
            }
            "shape_operation" => {
                let ids = ids_of(a)?;
                if ids.len() < 2 {
                    return Err("a shape operation needs at least two shapes".into());
                }
                let p = self.page(a)?;
                let how = a.get("how").and_then(Value::as_str).unwrap_or("");
                let op = ["union", "combine", "fragment", "intersect", "subtract", "join", "trim"].iter().position(|x| *x == how).ok_or_else(|| format!("unknown shape operation {how}"))? as i32 + 6;
                let result = self.engine.group_op(p, op, &ids.join("\n"))?;
                Ok(pretty(&serde_json::from_str::<Value>(&result).map_err(|e| e.to_string())?))
            }
            "list_connectors" => {
                let p = self.page(a)?;
                Ok(pretty(&serde_json::from_str::<Value>(&self.engine.connectors_json(p)?).map_err(|e| e.to_string())?))
            }
            "set_shape_name" => {
                let p = self.page(a)?;
                let id = a.get("id").map(id_text).unwrap_or_default();
                self.engine.page_op(40, p, 0, &format!("{id}|{}", a.get("name").map(id_text).unwrap_or_default()))?;
                let names: Value = serde_json::from_str(&self.engine.shape_names_json(p)?).map_err(|e| e.to_string())?;
                Ok(pretty(&json!({ "name": names[&id].as_str().unwrap_or("") })))
            }
            "set_alt_text" => {
                let p = self.page(a)?;
                self.engine.page_op(39, p, 0, &format!("{}|{}", a.get("id").map(id_text).unwrap_or_default(), a.get("text").map(id_text).unwrap_or_default()))?;
                Ok("ok".into())
            }
            "set_hyperlinks" => {
                let links = a.get("links").filter(|l| l.is_array()).ok_or("links must be a list")?;
                let p = self.page(a)?;
                let id = a.get("id").map(id_text).unwrap_or_default();
                self.engine.page_op(19, p, 0, &format!("{id}|{links}"))?;
                let now: Vec<Value> = serde_json::from_str(&self.engine.hyperlinks_json(p, &id)?).map_err(|e| e.to_string())?;
                Ok(format!("{} link(s)", now.len()))
            }
            "set_shape_data" => {
                let p = self.page(a)?;
                let id = a.get("id").map(id_text).unwrap_or_default();
                let name = a.get("name").cloned().unwrap_or(Value::Null);
                let label = match a.get("label") {
                    Some(Value::String(s)) if !s.is_empty() => json!(s),
                    _ => name.clone(),
                };
                let row = json!({ "name": name, "label": label, "value": a.get("value").map(id_text).unwrap_or_default(), "prompt": "", "format": "", "type": a.get("type").cloned().filter(|t| !t.is_null()).unwrap_or(json!(0)), "invisible": false });
                self.engine.edit(p, 17, &format!("{id}\n{row}"), 0.0, 0.0, 0.0, 0.0, 0)?;
                Ok("ok".into())
            }
            "set_page_size" => {
                let (w, h) = (num(a, "width")?, num(a, "height")?);
                if w <= 0.0 || h <= 0.0 {
                    return Err("the size must be positive".into());
                }
                let p = self.page(a)?;
                self.engine.page_op(6, p, 0, &format!("{},{}", js_num(w), js_num(h)))?;
                Ok("ok".into())
            }
            "rename_page" => {
                let p = self.page(a)?;
                self.engine.page_op(3, p, 0, &a.get("name").map(id_text).unwrap_or_default())?;
                Ok("ok".into())
            }
            "duplicate_page" => {
                let p = self.page(a)?;
                self.engine.page_op(5, p, 0, "")?;
                Ok(format!("pages: {}", self.engine.page_count().unwrap_or(0)))
            }
            "delete_page" => {
                if self.engine.page_count().unwrap_or(0) < 2 {
                    return Err("the last page cannot be deleted".into());
                }
                let p = self.page(a)?;
                self.engine.page_op(2, p, 0, "")?;
                Ok(format!("pages: {}", self.engine.page_count().unwrap_or(0)))
            }
            "add_layer" => {
                let p = self.page(a)?;
                self.engine.page_op(7, p, 0, &a.get("name").map(id_text).unwrap_or_default())?;
                Ok("ok".into())
            }
            "list_layers" => {
                let p = self.page(a)?;
                let model = self.model();
                let rows: Vec<Value> = model["pages"][p]["layers"].as_array().map_or_else(Vec::new, |l| {
                    l.iter().enumerate().map(|(index, l)| json!({ "index": index, "name": l["name"], "visible": truthy(&l["visible"]), "print": l["print"] != json!(false), "locked": truthy(&l["locked"]) })).collect()
                });
                Ok(pretty(&Value::Array(rows)))
            }
            "set_layer" => {
                let p = self.page(a)?;
                let at = self.layer_index(p, a.get("layer"))?;
                let mut changed = false;
                let tri = |k: &str, changed: &mut bool| match a.get(k) {
                    Some(v) if !v.is_null() => {
                        *changed = true;
                        if truthy(v) { "1" } else { "0" }
                    }
                    _ => "",
                };
                let (visible, print, locked) = (tri("visible", &mut changed), tri("print", &mut changed), tri("locked", &mut changed));
                let name = match a.get("newName") {
                    Some(v) if !v.is_null() => {
                        changed = true;
                        id_text(v)
                    }
                    _ => String::new(),
                };
                if changed {
                    self.engine.page_op(8, p, at, &format!("{name}|{visible}|{print}|{locked}"))?;
                }
                if let Some(color) = a.get("color").filter(|c| !c.is_null()) {
                    self.engine.page_op(10, p, at, &id_text(color))?;
                }
                Ok("ok".into())
            }
            "assign_layer" => {
                let p = self.page(a)?;
                let ids = ids_of(a)?;
                let at = self.layer_index(p, a.get("layer"))?;
                let member = a.get("member") != Some(&json!(false));
                self.engine.edit(p, 16, &ids.join("\n"), at as f64, 0.0, 0.0, 0.0, member as i32)?;
                Ok("ok".into())
            }
            "delete_layer" => {
                let p = self.page(a)?;
                let at = self.layer_index(p, a.get("layer"))?;
                self.engine.page_op(9, p, at, "")?;
                Ok("ok".into())
            }
            "list_themes" => {
                let t: Value = serde_json::from_str(&self.engine.themes_json()?).map_err(|e| e.to_string())?;
                let themes: Vec<Value> = t["themes"].as_array().map_or_else(Vec::new, |l| l.iter().map(|x| json!({ "name": x["name"], "accents": x["accents"], "fonts": x["fonts"] })).collect());
                Ok(pretty(&json!({ "current": t["current"], "variant": t["variant"], "themes": themes })))
            }
            "apply_theme" => {
                let name = a.get("name").map(id_text).unwrap_or_default();
                if a.get("page").is_none() {
                    self.engine.page_op(30, 0, 0, &name)?;
                } else {
                    let p = self.page(a)?;
                    self.engine.page_op(32, p, 0, &name)?;
                }
                let t: Value = serde_json::from_str(&self.engine.themes_json()?).map_err(|e| e.to_string())?;
                Ok(format!("theme {}", if t["current"].is_null() { "null".to_string() } else { id_text(&t["current"]) }))
            }
            "open_stencil" => {
                let st = match a.get("path").filter(|p| truthy(p)) {
                    Some(path) => {
                        let bytes = std::fs::read(self.inside(Some(path))?).map_err(|e| e.to_string())?;
                        self.engine.stencil_open(bytes)?
                    }
                    None => self.engine.stencil_builtin()?,
                };
                let st: Value = serde_json::from_str(&st).map_err(|e| e.to_string())?;
                let masters: Vec<Value> = st["masters"].as_array().map_or_else(Vec::new, |l| l.iter().map(|m| json!({ "id": m["id"], "name": m["name"] })).collect());
                Ok(pretty(&json!({ "stencil": st["index"], "masters": masters })))
            }
            "drop_master" => {
                let p = self.page(a)?;
                let stencil = a.get("stencil").and_then(Value::as_u64).ok_or("no such stencil")? as usize;
                let master = a.get("master").map(id_text).unwrap_or_default();
                let id = self.engine.stencil_drop(p, stencil, &master, num(a, "x")?, num(a, "y")?)?;
                Ok(pretty(&json!({ "id": id })))
            }
            "insert_picture" => {
                let p = self.page(a)?;
                let bytes = std::fs::read(self.inside(a.get("path"))?).map_err(|e| e.to_string())?;
                let id = self.engine.insert_picture(p, &bytes, num(a, "x0")?, num(a, "y0")?, num(a, "x1")?, num(a, "y1")?)?;
                Ok(pretty(&json!({ "id": id })))
            }
            "check_diagram" => {
                let all = a.get("allPages").is_some_and(truthy);
                let count = self.engine.page_count().unwrap_or(0);
                let pages: Vec<usize> = if all { (0..count).collect() } else { vec![self.page(a)?] };
                let model = self.model();
                let alt = a.get("altText").is_some_and(truthy);
                let mut issues = Vec::new();
                for &p in &pages {
                    let size: Value = serde_json::from_str(&self.engine.page_size_json(p)?).map_err(|e| e.to_string())?;
                    let size = size["w"].as_f64().zip(size["h"].as_f64());
                    for mut i in layoutcraft_engine::check::check_page(&model, p, size, alt) {
                        i.page = Some(p);
                        issues.push(i);
                    }
                }
                if let Some(csv) = a.get("csv").filter(|c| !c.is_null()) {
                    let names: Vec<String> = model["pages"].as_array().map_or_else(Vec::new, |l| l.iter().enumerate().map(|(k, q)| q["name"].as_str().filter(|n| !n.is_empty()).map_or_else(|| format!("Page-{}", k + 1), str::to_string)).collect());
                    let text = format!("\u{feff}{}", layoutcraft_engine::check::issues_to_csv(&issues, &names));
                    std::fs::write(self.inside(Some(csv))?, text).map_err(|e| e.to_string())?;
                }
                let rows: Vec<Value> = issues
                    .iter()
                    .map(|i| {
                        let mut o = serde_json::Map::new();
                        o.insert("kind".into(), json!(i.kind));
                        o.insert("id".into(), json!(i.id));
                        o.insert("message".into(), json!(i.message));
                        if let Some(f) = &i.fix {
                            o.insert("fix".into(), json!(f));
                        }
                        if all {
                            o.insert("page".into(), json!(i.page));
                        }
                        Value::Object(o)
                    })
                    .collect();
                Ok(pretty(&Value::Array(rows)))
            }
            "compare_files" => {
                let load = |p: Option<&Value>| -> Result<Engine, String> {
                    let full = self.inside(p)?;
                    let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
                    let mut e = Engine::new();
                    match full.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase).as_deref() {
                        Some("drawio") => {
                            e.new_blank(11.0, 8.5, false, "Page-1")?;
                            e.import_drawio(&bytes)?;
                        }
                        Some("excalidraw") => {
                            e.new_blank(11.0, 8.5, false, "Page-1")?;
                            e.import_excalidraw(&String::from_utf8_lossy(&bytes))?;
                        }
                        _ => e.open(bytes)?,
                    }
                    Ok(e)
                };
                let (new, old) = (load(a.get("file"))?, load(a.get("other"))?);
                let diffs = new.compare_with(&old)?;
                if let Some(csv) = a.get("csv").filter(|c| !c.is_null()) {
                    std::fs::write(self.inside(Some(csv))?, format!("\u{feff}{}", layoutcraft_engine::compare::diffs_to_csv(&diffs))).map_err(|e| e.to_string())?;
                }
                let rows: Vec<Value> = diffs.iter().map(|d| json!({ "page": d.page, "pageName": d.page_name, "kind": d.kind, "what": d.what, "id": d.id, "label": d.label, "detail": d.detail })).collect();
                Ok(pretty(&Value::Array(rows)))
            }
            "inventory" => {
                let all = a.get("allPages").is_some_and(truthy);
                let names: Vec<String> = serde_json::from_str(&self.engine.page_names_json()?).map_err(|e| e.to_string())?;
                let model = self.model();
                let p = if all { 0 } else { self.page(a)? };
                let (rows, data_cols) = layoutcraft_engine::inventory::inventory_data(&model, &names, p, all);
                let columns: Vec<String> = match a.get("columns") {
                    None => ["page", "id", "text"].iter().map(|s| s.to_string()).chain(data_cols.iter().map(|c| c.key.clone())).collect(),
                    Some(list) => list.as_array().map_or_else(Vec::new, |l| l.iter().map(id_text).collect()),
                };
                if let Some(csv) = a.get("csv").filter(|c| !c.is_null()) {
                    std::fs::write(self.inside(Some(csv))?, format!("\u{feff}{}", layoutcraft_engine::inventory::inventory_csv(&model, &names, p, &columns, all))).map_err(|e| e.to_string())?;
                }
                let cols: Vec<Value> = columns.iter().map(|k| json!({ "key": k, "label": layoutcraft_engine::inventory::label(&data_cols, k) })).collect();
                let table: Vec<Value> = rows
                    .iter()
                    .map(|r| Value::Array(columns.iter().map(|k| match k.as_str() { "id" => r.id.clone(), other => json!(layoutcraft_engine::inventory::cell(r, other)) }).collect()))
                    .collect();
                Ok(pretty(&json!({ "columns": cols, "rows": table })))
            }
            "find_shapes" => {
                let query = a.get("query").map(id_text);
                if query.as_deref().is_none_or(str::is_empty) && a.get("dataName").is_none() {
                    return Err("give a query or a dataName".into());
                }
                if a.get("dataValue").is_some() && a.get("dataName").is_none() {
                    return Err("dataValue needs a dataName".into());
                }
                let q = query.unwrap_or_default().to_lowercase();
                let same = |x: &Value, y: &str| js_lower(x) == y.to_lowercase();
                let count = self.engine.page_count().unwrap_or(0);
                let pages: Vec<usize> = if a.get("allPages").is_some_and(truthy) { (0..count).collect() } else { vec![self.page(a)?] };
                let model = self.model();
                let mut out = Vec::new();
                for p in pages {
                    let size: Value = serde_json::from_str(&self.engine.page_size_json(p)?).map_err(|e| e.to_string())?;
                    let names: Value = serde_json::from_str(&self.engine.shape_names_json(p)?).map_err(|e| e.to_string())?;
                    for s in model["pages"][p]["shapes"].as_array().cloned().unwrap_or_default() {
                        let row = shape_row(&s, size["h"].as_f64().unwrap_or(0.0), &names);
                        let data = s["data"].as_array().cloned().unwrap_or_default();
                        let hay = [row["text"].clone(), row["altText"].clone(), row["name"].clone()].into_iter().chain(data.iter().map(|d| d["value"].clone()));
                        let text_hit = q.is_empty() || hay.into_iter().any(|v| js_lower(&v).contains(&q));
                        let data_hit = match a.get("dataName") {
                            None => true,
                            Some(n) => {
                                let n = id_text(n);
                                data.iter().any(|d| same(&d["name"], &n) && a.get("dataValue").is_none_or(|v| same(&d["value"], &id_text(v))))
                            }
                        };
                        if text_hit && data_hit {
                            let mut o = serde_json::Map::new();
                            o.insert("page".into(), json!(p));
                            o.extend(row.as_object().cloned().unwrap_or_default());
                            out.push(Value::Object(o));
                        }
                    }
                }
                Ok(pretty(&Value::Array(out)))
            }
            "add_border" => {
                let styles = layoutcraft_engine::borders::BORDER_STYLES;
                let style = a.get("style").map(id_text).unwrap_or_else(|| "undefined".into());
                if !styles.iter().any(|(n, _)| *n == style) {
                    return Err(format!("unknown border style: {style} (one of {})", styles.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")));
                }
                let p = self.page(a)?;
                let id = self.engine.apply_border(p, &style, &a.get("title").map(id_text).unwrap_or_default())?;
                Ok(pretty(&json!({ "backgroundPage": id })))
            }
            "remove_border" => {
                let p = self.page(a)?;
                self.engine.remove_border(p)?;
                Ok("ok".into())
            }
            "get_page" => {
                let p = self.page(a)?;
                let size: Value = serde_json::from_str(&self.engine.page_size_json(p)?).map_err(|e| e.to_string())?;
                let names: Vec<String> = serde_json::from_str(&self.engine.page_names_json()?).map_err(|e| e.to_string())?;
                let model = self.model();
                let pg = &model["pages"][p];
                let bg: Value = serde_json::from_str(&self.engine.background_info_json()?).map_err(|e| e.to_string())?;
                let assigned = bg["assigned"][p].clone();
                let background = if truthy(&assigned) {
                    let name = bg["backgrounds"].as_array().and_then(|l| l.iter().find(|b| b["id"] == assigned)).map_or(Value::Null, |b| b["name"].clone());
                    json!({ "id": assigned, "name": name })
                } else {
                    Value::Null
                };
                let layers: Vec<Value> = pg["layers"].as_array().map_or_else(Vec::new, |l| l.iter().map(|l| json!({ "name": l["name"], "visible": l["visible"] != json!(false), "locked": truthy(&l["locked"]) })).collect());
                Ok(pretty(&json!({ "index": p, "name": names[p], "width": size["w"], "height": size["h"], "shapes": pg["shapes"].as_array().map_or(0, Vec::len), "connectors": pg["connectors"].as_array().map_or(0, Vec::len), "layers": layers, "background": background })))
            }
            "undo" => Ok(if self.engine.undo()? { "undone" } else { "nothing to undo" }.into()),
            "redo" => Ok(if self.engine.redo()? { "redone" } else { "nothing to redo" }.into()),
            _ if self.tool_exists(name) => Err(format!("the tool {name} is not in the native server yet")),
            _ => Err(format!("unknown tool {name}")),
        }
    }

    fn layer_index(&self, p: usize, name: Option<&Value>) -> Result<usize, String> {
        let model = self.model();
        let layers = model["pages"][p]["layers"].as_array().cloned().unwrap_or_default();
        let want = name.map(id_text).unwrap_or_else(|| "undefined".into());
        layers.iter().position(|l| l["name"].as_str() == Some(want.as_str())).ok_or_else(|| {
            let have: Vec<&str> = layers.iter().filter_map(|l| l["name"].as_str()).collect();
            format!("no layer named {want} (layers: {})", if have.is_empty() { "none".to_string() } else { have.join(", ") })
        })
    }

    fn tool_exists(&self, name: &str) -> bool {
        let list: Value = serde_json::from_str(TOOLS_LIST).unwrap_or(Value::Null);
        list["tools"].as_array().is_some_and(|t| t.iter().any(|x| x["name"] == name))
    }
}

fn shape_row(s: &Value, page_h: f64, names: &Value) -> Value {
    let f = |k: &str| s[k].as_f64().unwrap_or(0.0);
    let id = match &s["visioId"] {
        Value::String(t) => t.clone(),
        other => other.to_string(),
    };
    let rot = f("rot");
    let rotation = if rot != 0.0 { (-rot * 1e6).round() / 1e6 } else { 0.0 };
    json!({
        "id": id, "type": s["type"], "x": inch(f("x")), "y": inch(page_h * PX_PER_INCH - f("y") - f("h")), "w": inch(f("w")), "h": inch(f("h")),
        "rotation": rotation, "text": s["label"].as_str().unwrap_or(""), "name": names[&id].as_str().unwrap_or(""), "altText": s["altText"].as_str().unwrap_or(""),
        "fill": s["fill"], "line": s["stroke"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_list_is_the_checked_in_one() {
        let mut s = Server::new(std::env::temp_dir());
        let got = s.handle(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap()["result"].clone();
        let want: Value = serde_json::from_str(TOOLS_LIST).unwrap();
        assert_eq!(got, want);
        assert_eq!(want["tools"].as_array().unwrap().len(), 49);
    }

    fn call(s: &mut Server, name: &str, args: Value) -> (bool, String) {
        let r = s.handle(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}})).unwrap()["result"].clone();
        (r["isError"] == json!(true), r["content"][0]["text"].as_str().unwrap_or("").to_string())
    }

    #[test]
    fn every_listed_tool_is_implemented() {
        let list: Value = serde_json::from_str(TOOLS_LIST).unwrap();
        let mut s = Server::new(std::env::temp_dir());
        for t in list["tools"].as_array().unwrap() {
            let (_, text) = call(&mut s, t["name"].as_str().unwrap(), json!({}));
            assert!(!text.contains("not in the native server yet") && !text.starts_with("unknown tool"), "{}: {text}", t["name"]);
        }
        assert!(call(&mut s, "no_such_tool", json!({})).1.starts_with("unknown tool"));
    }

    #[test]
    fn draw_list_and_the_root_folder() {
        let root = std::env::temp_dir().join(format!("lcmcp-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut s = Server::new(root.clone());
        let (err, text) = call(&mut s, "draw_shape", json!({"kind":"rectangle","x0":1,"y0":1,"x1":3,"y1":2,"text":"Hi"}));
        assert!(!err, "{text}");
        let shapes: Value = serde_json::from_str(&call(&mut s, "list_shapes", json!({})).1).unwrap();
        assert_eq!(shapes.as_array().unwrap().len(), 1);
        assert_eq!(shapes[0]["text"], "Hi");
        assert!((shapes[0]["w"].as_f64().unwrap() - 2.0).abs() < 1e-6);
        assert!(call(&mut s, "export", json!({"format":"svg","path":"../escape.svg"})).0);
        let (err, text) = call(&mut s, "export", json!({"format":"drawio","path":"out/a.drawio"}));
        assert!(!err && text.starts_with("wrote out/a.drawio"), "{text}");
        assert!(root.join("out/a.drawio").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_path_outside_the_root_is_refused() {
        let s = Server::new(std::env::temp_dir().join("lcroot"));
        assert!(s.inside(Some(&json!("../x"))).is_err());
        assert!(s.inside(Some(&json!("a/../../x"))).is_err());
        assert!(s.inside(Some(&json!("a/b.vsdx"))).is_ok());
        assert!(s.inside(None).is_err());
    }
}

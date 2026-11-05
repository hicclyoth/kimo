//! kimo: command-line front end over the same core dispatch as the MCP server (no second dispatch path).
//! Exit codes: 0 ok, 1 tool error, 2 usage, 3 preflight failed. Results go to stdout, errors and progress to stderr.
mod gui;
use kimo_core::commands::{registry_ref, Session};
use kimo_core::toolgroups;
use serde_json::{json, Value};
use std::io::Read;
use std::sync::{Arc, Mutex};

#[global_allocator]
static ALLOC: kimo_core::guard::Counting = kimo_core::guard::Counting;

const EXIT_OK: i32 = 0;
const EXIT_TOOL: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_PREFLIGHT: i32 = 3;

const USAGE: &str = "usage: kimo [--json] [--quiet|-q] <command> ...
  call <tool> [--args '{...}'|-] [--project p.kimo] [--no-save]
  export <project.kimo> <out> [--preset name] [--range a:b | --start s --end e] [--crf n]
  gui [--project p.kimo] [--port 4567] [--bind 127.0.0.1]
  preflight [--project p.kimo] [--args '{...}']
  validate [--project p.kimo] [--args '{...}']
  render-job <sub> [--project p.kimo] [--args '{...}']
  info | tools [search <query> | describe <tool>] | --version
tool arguments: --args '{...}' (or '-' to read them from stdin); `--json '{...}'` is accepted as an alias. A bare --json means machine output
  (one compact JSON document on stdout, errors as {\"error\":{...}} on stderr). Progress lines (\"kimo: ...\") go to stderr only; -q silences them.
state: `call` without --project is stateless (one empty in-memory project, nothing is saved). With --project the file is loaded if it exists, and
  saved atomically after a call that changes the project (--no-save to skip); a missing file is created, with \"kimo: created <path>\" on stderr.
exit codes: 0 ok, 1 tool error, 2 usage, 3 preflight/delivery check failed, 124 call hung and could not be cancelled (process exits)";

/// A failure carrying its exit code and the structured error body.
struct Fail { code: i32, body: Value }
fn usage_err(m: impl Into<String>) -> Fail { Fail { code: EXIT_USAGE, body: json!({"code":"usage","message":m.into()}) } }
fn tool_err(e: kimo_core::errors::ToolError) -> Fail { Fail { code: EXIT_TOOL, body: serde_json::to_value(&e).unwrap_or(json!({"code":e.code,"message":e.message})) } }

struct Opts { json_out: bool, quiet: bool }

/// Runs one tool exactly as the MCP server does: group resolution, worker thread with cancel token, wall-clock limit, same ToolError.
pub(crate) fn run_tool(ss: &Arc<Mutex<Session>>, name: &str, args: &Value) -> Result<Value, kimo_core::errors::ToolError> {
    let (name, args) = if toolgroups::is_group(name) { toolgroups::resolve_group_call(name, args).map_err(kimo_core::errors::ToolError::from)? } else { (name.to_string(), args.clone()) };
    match kimo_core::guard::run_tool_guarded(ss, &name, &args) {
        kimo_core::guard::Guarded::Done(r) => r,
        kimo_core::guard::Guarded::Hung { limit } => { eprintln!("{}", json!({"error":{"code":"timeout","message":format!("tool '{name}' hung past {limit}s and could not be cancelled; exiting (exit code 124)")}})); std::process::exit(124); }
    }
}

/// Never print inline image payloads (base64 content blocks): keep type/mimeType/path and say the bytes were omitted.
fn strip_images(v: &mut Value) {
    match v {
        Value::Object(m) => {
            let is_img = m.get("type").and_then(|t| t.as_str()) == Some("image") || m.contains_key("image_base64");
            if is_img { for k in ["data", "image_base64", "base64"] { if m.remove(k).is_some() { m.insert("data_omitted".into(), json!(true)); } } }
            for x in m.values_mut() { strip_images(x); }
        }
        Value::Array(a) => { a.retain(|x| x.get("type").and_then(|t| t.as_str()) != Some("image") || x.get("path").is_some()); for x in a.iter_mut() { strip_images(x); } }
        _ => {}
    }
}

fn progress(o: &Opts, m: &str) { if !o.quiet { eprintln!("kimo: {m}"); } }

fn exists(name: &str) -> bool { registry_ref().iter().any(|c| c.name == name) }
/// Resolves a group call ("graphics" + action) to the real tool name first, so the mutates flag is read from the tool that actually runs.
fn mutates(name: &str, args: &Value) -> bool {
    let real = if toolgroups::is_group(name) { match toolgroups::resolve_group_call(name, args) { Ok((a, _)) => a, Err(_) => return false } } else { name.to_string() };
    registry_ref().iter().find(|c| c.name == real).map(|c| c.mutates).unwrap_or(false)
}

struct Parsed { pos: Vec<String>, flags: Vec<(String, Option<String>)> }
const VALUE_FLAGS: &[&str] = &["--args", "--project", "--preset", "--range", "--start", "--end", "--crf", "--encoder", "--limit", "--out-json", "--port", "--bind"];
fn parse(args: &[String]) -> Result<Parsed, Fail> {
    let (mut pos, mut flags) = (vec![], vec![]); let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--json" {
            // bare --json = machine output; `--json <value>` where value starts with { [ or is "-" = alias of --args (tool arguments)
            if let Some(n) = args.get(i + 1) { if n == "-" || n.starts_with('{') || n.starts_with('[') { flags.push(("--args".into(), Some(n.clone()))); i += 2; continue; } }
            flags.push(("--json".into(), None));
        } else if a == "-q" { flags.push(("--quiet".into(), None)); } else if a == "-V" || a == "--version" { pos.push("--version".into()); } else if let Some((k, v)) = a.strip_prefix("--").and_then(|r| r.split_once('=')).map(|(k, v)| (format!("--{k}"), v.to_string())) { flags.push((k, Some(v))); }
        else if a.starts_with("--") { if VALUE_FLAGS.contains(&a.as_str()) { let v = args.get(i + 1).ok_or_else(|| usage_err(format!("{a} needs a value")))?; flags.push((a.clone(), Some(v.clone()))); i += 1; } else { flags.push((a.clone(), None)); } }
        else { pos.push(a.clone()); }
        i += 1;
    }
    Ok(Parsed { pos, flags })
}
impl Parsed {
    fn flag(&self, k: &str) -> Option<&Option<String>> { self.flags.iter().find(|(f, _)| f == k).map(|(_, v)| v) }
    fn has(&self, k: &str) -> bool { self.flag(k).is_some() }
    fn val(&self, k: &str) -> Option<String> { self.flag(k).and_then(|v| v.clone()) }
    fn args_json(&self) -> Result<Value, Fail> {
        match self.val("--args") {
            None => Ok(json!({})),
            Some(s) => { let txt = if s == "-" { let mut b = String::new(); std::io::stdin().read_to_string(&mut b).map_err(|e| usage_err(e.to_string()))?; b } else { s };
                let v: Value = serde_json::from_str(&txt).map_err(|e| usage_err(format!("--args is not valid JSON: {e}")))?;
                if !v.is_object() { return Err(usage_err("--args must be a JSON object")); } Ok(v) }
        }
    }
}

fn load(ss: &Arc<Mutex<Session>>, p: &str, o: &Opts) -> Result<bool, Fail> {
    if std::path::Path::new(p).exists() { progress(o, &format!("loading {p}")); run_tool(ss, "load_project", &json!({"path":p})).map_err(tool_err)?; return Ok(true); }
    Ok(false)
}
fn save(ss: &Arc<Mutex<Session>>, p: &str, o: &Opts) -> Result<(), Fail> { progress(o, &format!("saving {p}")); run_tool(ss, "save_project", &json!({"path":p})).map(|_| ()).map_err(tool_err) }

/// Call a tool, with optional project load before and atomic save after a mutating call.
fn call(ss: &Arc<Mutex<Session>>, tool: &str, args: Value, project: Option<&str>, save_after: bool, o: &Opts) -> Result<Value, Fail> {
    let existed = match project { Some(p) => load(ss, p, o)?, None => false };
    progress(o, &format!("running {tool}")); // TODO(RV-6): structured NDJSON progress events on stderr
    let will_save = save_after && mutates(tool, &args);
    let mut r = run_tool(ss, tool, &args).map_err(tool_err)?;
    strip_images(&mut r);
    if let (Some(p), true) = (project, will_save) { if !existed && !o.quiet { eprintln!("kimo: created {p}"); } save(ss, p, o)?; }
    Ok(r)
}

fn run(p: Parsed, o: &Opts) -> Result<(i32, Value), Fail> {
    let ss = Arc::new(Mutex::new(Session::new()));
    let cmd = p.pos.first().map(|s| s.as_str()).ok_or_else(|| usage_err("missing command"))?;
    let project = p.val("--project");
    let save_after = !p.has("--no-save");
    match cmd {
        "call" => {
            let tool = p.pos.get(1).ok_or_else(|| usage_err("call needs a tool name"))?; if p.pos.len() > 2 { return Err(usage_err("call takes one tool name")); }
            let args = p.args_json()?;
            Ok((EXIT_OK, call(&ss, tool, args, project.as_deref(), save_after, o)?))
        }
        "export" => {
            let (proj, out) = (p.pos.get(1).ok_or_else(|| usage_err("export needs <project.kimo> <out>"))?, p.pos.get(2).ok_or_else(|| usage_err("export needs <project.kimo> <out>"))?);
            if p.pos.len() > 3 { return Err(usage_err("export takes <project.kimo> <out>")); }
            let mut a = json!({"path":out});
            let num = |k: &str, s: &str| -> Result<f64, Fail> { s.parse::<f64>().map_err(|_| usage_err(format!("{k} needs a number, got '{s}'"))) };
            if let Some(r) = p.val("--range") { let (s, e) = r.split_once(':').ok_or_else(|| usage_err("--range wants start:end seconds"))?; if !s.is_empty() { a["start"] = json!(num("--range start", s)?); } if !e.is_empty() { a["end"] = json!(num("--range end", e)?); } }
            if let Some(s) = p.val("--start") { a["start"] = json!(num("--start", &s)?); }
            if let Some(s) = p.val("--end") { a["end"] = json!(num("--end", &s)?); }
            if let Some(e) = p.val("--encoder") { a["encoder"] = json!(e); }
            let tool = if let Some(pr) = p.val("--preset") { a["preset"] = json!(pr); "export_preset" } else { if let Some(c) = p.val("--crf") { a["crf"] = json!(num("--crf", &c)?); } "export" };
            if p.has("--crf") && tool == "export_preset" { return Err(usage_err("--crf cannot be combined with --preset")); }
            if !std::path::Path::new(proj).exists() { return Err(Fail { code: EXIT_TOOL, body: json!({"code":"io_error","message":format!("cannot read '{proj}': no such file")}) }); }
            Ok((EXIT_OK, call(&ss, tool, a, Some(proj), false, o)?))
        }
        "preflight" | "validate" | "render-job" => {
            let (tool, args) = match cmd {
                "preflight" => ("preflight".to_string(), p.args_json()?),
                "validate" => ("validate_export".to_string(), p.args_json()?),
                _ => { let sub = p.pos.get(1).ok_or_else(|| usage_err("render-job needs a subcommand"))?; (format!("render_job_{}", sub.replace('-', "_")), p.args_json()?) }
            };
            if !exists(&tool) { return Err(Fail { code: EXIT_TOOL, body: json!({"code":"tool_unavailable","message":format!("tool '{tool}' is not available in this build yet"),"param":"tool"}) }); }
            match call(&ss, &tool, args, project.as_deref(), save_after, o) {
                Ok(v) => { let failed = (cmd == "preflight" && v.get("ok") == Some(&json!(false))) || (cmd == "validate" && v.get("status").and_then(|x| x.as_str()) == Some("fail"));
                    if failed && cmd == "validate" { progress(o, "delivery check failed"); } Ok((if failed { EXIT_PREFLIGHT } else { EXIT_OK }, v)) }
                Err(f) if cmd == "preflight" && f.body.get("code") == Some(&json!("preflight_failed")) => Err(Fail { code: EXIT_PREFLIGHT, body: f.body }),
                Err(f) => Err(f),
            }
        }
        "gui" => {
            if let Some(p) = &project { load(&ss, p, o)?; }
            let bind = p.val("--bind").unwrap_or_else(|| "127.0.0.1".to_string());
            let port: u16 = p.val("--port").map(|s| s.parse::<u16>().map_err(|_| usage_err("--port needs a number 0..65535"))).transpose()?.unwrap_or(4567);
            progress(o, &format!("gui listening on http://{bind}:{port}"));
            gui::serve(ss.clone(), &bind, port).map_err(|e| Fail { code: EXIT_TOOL, body: json!({"code":"io_error","message":format!("gui server: {e}")}) })?;
            Ok((EXIT_OK, json!({"ok": true})))
        }
        "info" => Ok((EXIT_OK, call(&ss, "kimo_info", json!({}), None, false, o)?)),
        "tools" => match p.pos.get(1).map(|s| s.as_str()) {
            None => Ok((EXIT_OK, json!({"tools": registry_ref().iter().map(|c| c.name).collect::<Vec<_>>()}))),
            Some("search") => { let q = p.pos[2..].join(" "); if q.is_empty() { return Err(usage_err("tools search needs a query")); }
                let lim = p.val("--limit").map(|s| s.parse::<usize>().map_err(|_| usage_err("--limit needs a number"))).transpose()?.unwrap_or(15); Ok((EXIT_OK, toolgroups::search(&q, lim))) }
            Some("describe") => { let a = p.pos.get(2).ok_or_else(|| usage_err("tools describe needs a tool name"))?; toolgroups::describe(&json!({"action":a})).map(|v| (EXIT_OK, v)).map_err(|e| tool_err(e.into())) }
            Some(x) => Err(usage_err(format!("unknown tools subcommand '{x}'"))),
        },
        "--version" => { let i = call(&ss, "kimo_info", json!({}), None, false, &Opts { json_out: o.json_out, quiet: true })?; let (v, c) = (i["version"].as_str().unwrap_or("?"), i["commit"].as_str().unwrap_or("?")); Ok((EXIT_OK, json!({"version":v,"commit":c}))) }
        "help" | "-h" | "--help" => Err(usage_err("")),
        x => Err(usage_err(format!("unknown command '{x}'"))),
    }
}

fn main() {
    kimo_core::guard::init();
    std::panic::set_hook(Box::new(|i| { eprintln!("kimo: {i}"); }));
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let parsed = parse(&argv);
    let (json_out, quiet) = parsed.as_ref().map(|p| (p.has("--json"), p.has("--quiet"))).unwrap_or((false, false));
    let o = Opts { json_out, quiet };
    let (code, res) = match parsed.and_then(|p| run(p, &o)) { Ok((c, v)) => (c, Ok(v)), Err(f) => (f.code, Err(f.body)) };
    match res {
        Ok(v) => { println!("{}", if o.json_out { serde_json::to_string(&v).unwrap() } else { serde_json::to_string_pretty(&v).unwrap() }); std::process::exit(code); }
        Err(b) => {
            if code == EXIT_USAGE && b["message"].as_str() == Some("") { eprintln!("{USAGE}"); std::process::exit(EXIT_USAGE); }
            if o.json_out { eprintln!("{}", serde_json::to_string(&json!({"error": b})).unwrap()); }
            else { eprintln!("kimo: error: {}", b["message"].as_str().unwrap_or("")); if let Some(h) = b["hint"].as_str() { eprintln!("hint: {h}"); } if code == EXIT_USAGE { eprintln!("{USAGE}"); } }
            std::process::exit(code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn strips_base64_image_blocks() {
        let mut v = json!({"content":[{"type":"image","data":"AAAA","mimeType":"image/png"},{"type":"text","text":"x"}],"shot":{"type":"image","data":"BBBB","path":"/tmp/a.png"},"image_base64":"CCCC"});
        strip_images(&mut v); let s = v.to_string(); assert!(!s.contains("AAAA") && !s.contains("BBBB") && !s.contains("CCCC"), "{s}");
        assert_eq!(v["shot"]["path"], "/tmp/a.png"); assert_eq!(v["content"].as_array().unwrap().len(), 1);
    }
}

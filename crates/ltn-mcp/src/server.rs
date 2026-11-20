//! JSON-RPC 2.0 / MCP message handling and the stdio loop.

use ltn_state::{Ctx, ToolDef};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Protocol versions this server speaks, oldest first.
pub const PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Most `tools/call` requests running at once; further calls get a
/// "server busy" error instead of queueing without bound.
pub const MAX_IN_FLIGHT: usize = 8;

/// JSON-RPC server error code used when every worker slot is taken.
const SERVER_BUSY: i64 = -32000;

/// Read JSON-RPC messages from stdin, one per line, and answer on stdout
/// until stdin closes.
pub fn serve_stdio(ctx: Ctx) {
    let tools = crate::all_tools();
    eprintln!(
        "ltn mcp: serving {} tools over stdio (state dir {})",
        tools.len(),
        ctx.home.display()
    );
    let stdin = std::io::stdin();
    let out: SharedWriter = Arc::new(Mutex::new(Box::new(std::io::stdout())));
    serve(ctx, tools, stdin.lock(), out, MAX_IN_FLIGHT);
}

/// Where replies go. One mutex for the whole stream, so concurrent replies
/// never interleave within a line.
pub type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// Write one reply line. Returns false once the stream is closed.
fn write_line(out: &SharedWriter, line: &str) -> bool {
    let mut w = match out.lock() {
        Ok(w) => w,
        Err(poisoned) => poisoned.into_inner(),
    };
    writeln!(w, "{line}").and_then(|_| w.flush()).is_ok()
}

/// True for a request that runs a tool (and so may be slow). Everything else
/// (initialize, tools/list, ping, notifications, malformed input) is answered
/// inline, in order.
fn is_tool_call(raw: &str) -> bool {
    match serde_json::from_str::<Value>(raw) {
        Ok(v) => v.get("id").is_some() && v.get("method").and_then(Value::as_str) == Some("tools/call"),
        Err(_) => false,
    }
}

/// The server loop over any line reader. Each `tools/call` runs on its own
/// thread (at most `max_in_flight` at once) so a slow tool such as an image
/// build does not block `ltn_status` or `ltn_logs`. Per-app mutations are
/// serialised by the engine's busy lock, not here. Returns after the reader
/// ends and every running call has written its reply.
pub fn serve<R: BufRead>(ctx: Ctx, tools: Vec<ToolDef>, reader: R, out: SharedWriter, max_in_flight: usize) {
    let ctx = Arc::new(ctx);
    let tools = Arc::new(tools);
    let in_flight = Arc::new(AtomicUsize::new(0));
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("ltn mcp: stdin read failed, stopping: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        workers.retain(|h| !h.is_finished());
        if !is_tool_call(&line) {
            if let Some(reply) = handle_message(&ctx, &tools, &line) {
                if !write_line(&out, &reply) {
                    eprintln!("ltn mcp: stdout closed, stopping");
                    break;
                }
            }
            continue;
        }
        // Reserve a slot; refuse rather than queue when all are busy.
        if in_flight.fetch_add(1, Ordering::SeqCst) >= max_in_flight {
            in_flight.fetch_sub(1, Ordering::SeqCst);
            let id = serde_json::from_str::<Value>(&line).ok().and_then(|v| v.get("id").cloned()).unwrap_or(Value::Null);
            let reply = error_reply(
                id,
                SERVER_BUSY,
                &format!("server busy: {max_in_flight} tool calls are already running; retry when one finishes"),
            );
            if !write_line(&out, &reply) {
                break;
            }
            continue;
        }
        let id = serde_json::from_str::<Value>(&line).ok().and_then(|v| v.get("id").cloned()).unwrap_or(Value::Null);
        let (w_ctx, w_tools, w_out, slot) = (ctx.clone(), tools.clone(), out.clone(), in_flight.clone());
        let spawned = std::thread::Builder::new().name("ltn-tool".into()).spawn(move || {
            // handle_message already turns handler panics into isError results.
            if let Some(reply) = handle_message(&w_ctx, &w_tools, &line) {
                if !write_line(&w_out, &reply) {
                    eprintln!("ltn mcp: stdout closed while replying");
                }
            }
            slot.fetch_sub(1, Ordering::SeqCst);
        });
        match spawned {
            Ok(h) => workers.push(h),
            Err(e) => {
                in_flight.fetch_sub(1, Ordering::SeqCst);
                eprintln!("ltn mcp: could not start a worker thread: {e}");
                if !write_line(&out, &error_reply(id, SERVER_BUSY, &format!("could not start a worker thread: {e}"))) {
                    break;
                }
            }
        }
    }
    for h in workers {
        let _ = h.join();
    }
}

/// Handle one raw line. Returns the reply to write, or None for notifications
/// (and for responses sent by the client, which this server never requests).
pub fn handle_message(ctx: &Ctx, tools: &[ToolDef], raw: &str) -> Option<String> {
    let msg: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => return Some(error_reply(Value::Null, PARSE_ERROR, &format!("parse error: {e}"))),
    };
    let Some(obj) = msg.as_object() else {
        return Some(error_reply(
            Value::Null,
            INVALID_REQUEST,
            "expected a JSON-RPC request object (batches are not supported)",
        ));
    };
    let id = obj.get("id").cloned();
    let Some(id) = id else {
        return None; // notification (e.g. notifications/initialized): never answered
    };
    let Some(method) = obj.get("method") else {
        if obj.contains_key("result") || obj.contains_key("error") {
            return None; // a response from the client; nothing to answer
        }
        return Some(error_reply(id, INVALID_REQUEST, "missing `method`"));
    };
    let Some(method) = method.as_str() else {
        return Some(error_reply(id, INVALID_REQUEST, "`method` must be a string"));
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);

    let outcome: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools.iter().map(describe).collect::<Vec<_>>() })),
        "tools/call" => call_tool(ctx, tools, &params),
        other => Err((METHOD_NOT_FOUND, format!("method not found: {other}"))),
    };
    Some(match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
        Err((code, message)) => error_reply(id, code, &message),
    })
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSIONS[PROTOCOL_VERSIONS.len() - 1]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "ltn", "version": env!("CARGO_PKG_VERSION") },
    })
}

fn describe(t: &ToolDef) -> Value {
    json!({
        "name": t.name,
        "description": t.description,
        "inputSchema": t.input_schema,
    })
}

fn call_tool(ctx: &Ctx, tools: &[ToolDef], params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "tools/call needs a string `name`".to_string()))?;
    let tool = tools
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| (INVALID_PARAMS, format!("unknown tool: {name}")))?;
    let args = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(v) if v.is_object() => v.clone(),
        Some(_) => return Err((INVALID_PARAMS, "`arguments` must be an object".into())),
    };

    let outcome = catch_unwind(AssertUnwindSafe(|| (tool.handler)(ctx, &args)));
    let (text, is_error) = match outcome {
        Ok(Ok(value)) => (
            serde_json::to_string_pretty(&value).unwrap_or_else(|e| format!("unserialisable result: {e}")),
            false,
        ),
        Ok(Err(e)) => (format!("{e:#}"), true),
        Err(panic) => {
            let what = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic payload".into());
            eprintln!("ltn mcp: tool {name} panicked: {what}");
            (format!("internal error: tool `{name}` panicked: {what}"), true)
        }
    };
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    }))
}

fn error_reply(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, Result};

    fn ok_handler(_: &Ctx, args: &Value) -> Result<Value> {
        Ok(json!({ "echo": args.clone() }))
    }
    fn err_handler(_: &Ctx, _: &Value) -> Result<Value> {
        Err(anyhow!("inner cause").context("outer failure"))
    }
    fn panic_handler(_: &Ctx, _: &Value) -> Result<Value> {
        panic!("boom in handler")
    }

    fn fake_tools() -> Vec<ToolDef> {
        let schema = json!({ "type": "object", "properties": {} });
        vec![
            ToolDef { name: "ok_tool", description: "returns its args", input_schema: schema.clone(), handler: ok_handler },
            ToolDef { name: "err_tool", description: "always fails", input_schema: schema.clone(), handler: err_handler },
            ToolDef { name: "panic_tool", description: "panics", input_schema: schema, handler: panic_handler },
        ]
    }

    fn ctx() -> Ctx {
        Ctx::for_home(&std::env::temp_dir().join("ltn-mcp-proto-test"))
    }

    fn send(raw: &str) -> Value {
        let reply = handle_message(&ctx(), &fake_tools(), raw).expect("expected a reply");
        serde_json::from_str(&reply).unwrap()
    }

    #[test]
    fn initialize_negotiates_version() {
        for v in PROTOCOL_VERSIONS {
            let r = send(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":v}}).to_string());
            assert_eq!(r["result"]["protocolVersion"], *v);
        }
        let r = send(r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#);
        assert_eq!(r["id"], 2);
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["capabilities"], json!({"tools":{}}));
        assert_eq!(r["result"]["serverInfo"]["name"], "ltn");
        assert_eq!(r["result"]["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
        let r = send(r#"{"jsonrpc":"2.0","id":3,"method":"initialize"}"#);
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn notifications_get_no_reply() {
        let t = fake_tools();
        assert!(handle_message(&ctx(), &t, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(handle_message(&ctx(), &t, r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#).is_none());
        assert!(handle_message(&ctx(), &t, r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"ok_tool"}}"#).is_none());
    }

    #[test]
    fn ping_returns_empty_result() {
        let r = send(r#"{"jsonrpc":"2.0","id":"p","method":"ping"}"#);
        assert_eq!(r["id"], "p");
        assert_eq!(r["result"], json!({}));
    }

    #[test]
    fn tools_list_shape() {
        let r = send(r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#);
        let tools = r["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["name"], "ok_tool");
        assert_eq!(tools[0]["description"], "returns its args");
        assert_eq!(tools[0]["inputSchema"]["type"], "object");
        assert!(tools.iter().all(|t| t.as_object().unwrap().len() == 3));
    }

    #[test]
    fn tools_call_ok_returns_pretty_json_text() {
        let r = send(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"ok_tool","arguments":{"a":1}}}"#);
        assert_eq!(r["result"]["isError"], false);
        let content = &r["result"]["content"][0];
        assert_eq!(content["type"], "text");
        let text = content["text"].as_str().unwrap();
        assert!(text.contains('\n'), "expected pretty JSON: {text}");
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), json!({"echo":{"a":1}}));
        // Missing arguments are treated as {}.
        let r = send(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"ok_tool"}}"#);
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), json!({"echo":{}}));
    }

    #[test]
    fn tools_call_err_is_an_error_result_with_chain() {
        let r = send(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"err_tool","arguments":{}}}"#);
        assert!(r.get("error").is_none());
        assert_eq!(r["result"]["isError"], true);
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("outer failure") && text.contains("inner cause"), "{text}");
    }

    #[test]
    fn tools_call_panic_is_an_error_result_and_server_survives() {
        let t = fake_tools();
        let c = ctx();
        let raw = r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"panic_tool"}}"#;
        let r: Value = serde_json::from_str(&handle_message(&c, &t, raw).unwrap()).unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("boom in handler"));
        let raw = r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#;
        let r: Value = serde_json::from_str(&handle_message(&c, &t, raw).unwrap()).unwrap();
        assert_eq!(r["result"], json!({}));
    }

    #[test]
    fn unknown_tool_is_invalid_params() {
        let r = send(r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"nope"}}"#);
        assert_eq!(r["id"], 10);
        assert_eq!(r["error"]["code"], -32602);
        assert!(r["error"]["message"].as_str().unwrap().contains("nope"));
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let r = send(r#"{"jsonrpc":"2.0","id":11,"method":"resources/list"}"#);
        assert_eq!(r["error"]["code"], -32601);
        assert_eq!(r["id"], 11);
    }

    #[test]
    fn parse_error_has_null_id() {
        let r = send("{not json");
        assert_eq!(r["error"]["code"], -32700);
        assert!(r.as_object().unwrap().contains_key("id"));
        assert!(r["id"].is_null());
    }

    fn sleep_handler(_: &Ctx, args: &Value) -> Result<Value> {
        let ms = args.get("ms").and_then(Value::as_u64).unwrap_or(2000);
        std::thread::sleep(std::time::Duration::from_millis(ms));
        Ok(json!({ "slept_ms": ms }))
    }

    fn concurrent_tools() -> Vec<ToolDef> {
        let mut t = fake_tools();
        t.push(ToolDef {
            name: "sleep_tool",
            description: "sleeps",
            input_schema: json!({ "type": "object", "properties": {} }),
            handler: sleep_handler,
        });
        t
    }

    /// A Write that appends into a shared buffer the test can read afterwards.
    #[derive(Clone)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Run the real serve loop over `input` lines; returns reply lines in write order.
    fn run_serve(input: &[String], max: usize) -> (Vec<Value>, std::time::Duration) {
        let buf = Buf(Arc::new(Mutex::new(Vec::new())));
        let out: SharedWriter = Arc::new(Mutex::new(Box::new(buf.clone())));
        let reader = std::io::Cursor::new(input.join("
") + "
");
        let start = std::time::Instant::now();
        serve(ctx(), concurrent_tools(), reader, out, max);
        let took = start.elapsed();
        let text = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        let lines = text
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap_or_else(|e| panic!("not one JSON object per line: {e}: {l}")))
            .collect();
        (lines, took)
    }

    fn call(id: u64, name: &str, args: Value) -> String {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string()
    }

    #[test]
    fn slow_tool_does_not_block_a_fast_one() {
        let input = vec![
            call(1, "sleep_tool", json!({"ms": 2000})),
            call(2, "ok_tool", json!({"x": 1})),
            r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#.to_string(),
        ];
        let (lines, took) = run_serve(&input, MAX_IN_FLIGHT);
        let ids: Vec<u64> = lines.iter().map(|l| l["id"].as_u64().unwrap()).collect();
        assert_eq!(ids.len(), 3, "{lines:?}");
        assert_eq!(*ids.last().unwrap(), 1, "the slow call must finish last: {ids:?}");
        assert!(ids[..2].contains(&2) && ids[..2].contains(&3), "{ids:?}");
        // serve waits for the slow call before returning.
        assert!(took >= std::time::Duration::from_millis(1900), "{took:?}");
        assert_eq!(lines[2]["result"]["isError"], false);
    }

    #[test]
    fn slow_calls_run_in_parallel() {
        let input: Vec<String> = (1..=4).map(|i| call(i, "sleep_tool", json!({"ms": 1500}))).collect();
        let (lines, took) = run_serve(&input, MAX_IN_FLIGHT);
        assert_eq!(lines.len(), 4);
        assert!(took < std::time::Duration::from_millis(4000), "4 x 1.5 s calls took {took:?}; not parallel");
    }

    #[test]
    fn concurrent_replies_are_whole_lines_with_matching_ids() {
        let big = "y".repeat(20_000);
        let input: Vec<String> = (1..=40).map(|i| call(i, "ok_tool", json!({"i": i, "pad": big}))).collect();
        // Cap above the request count so every call runs (the cap has its own test).
        let (lines, _) = run_serve(&input, 64);
        assert_eq!(lines.len(), 40);
        let mut ids: Vec<u64> = Vec::new();
        for l in &lines {
            let id = l["id"].as_u64().unwrap();
            // The reply carries the args of its own request, so ids were not mixed up.
            let text = l["result"]["content"][0]["text"].as_str().unwrap();
            let v: Value = serde_json::from_str(text).unwrap();
            assert_eq!(v["echo"]["i"].as_u64().unwrap(), id);
            ids.push(id);
        }
        ids.sort_unstable();
        assert_eq!(ids, (1..=40).collect::<Vec<_>>());
    }

    #[test]
    fn calls_over_the_cap_get_a_busy_error_not_a_hang() {
        let input: Vec<String> = (1..=3).map(|i| call(i, "sleep_tool", json!({"ms": 1000}))).collect();
        let (lines, _) = run_serve(&input, 2);
        assert_eq!(lines.len(), 3);
        let busy: Vec<&Value> = lines.iter().filter(|l| l.get("error").is_some()).collect();
        assert_eq!(busy.len(), 1, "{lines:?}");
        assert_eq!(busy[0]["id"], 3);
        assert_eq!(busy[0]["error"]["code"], -32000);
        assert!(busy[0]["error"]["message"].as_str().unwrap().contains("busy"));
    }

    #[test]
    fn panicking_call_on_a_worker_still_replies() {
        let input = vec![call(1, "panic_tool", json!({})), call(2, "ok_tool", json!({}))];
        let (lines, _) = run_serve(&input, MAX_IN_FLIGHT);
        let p = lines.iter().find(|l| l["id"] == 1).unwrap();
        assert_eq!(p["result"]["isError"], true);
        assert!(lines.iter().any(|l| l["id"] == 2 && l["result"]["isError"] == false));
    }

    #[test]
    fn client_responses_are_ignored() {
        assert!(handle_message(&ctx(), &fake_tools(), r#"{"jsonrpc":"2.0","id":1,"result":{}}"#).is_none());
    }
}

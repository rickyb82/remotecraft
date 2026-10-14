use remotecraft_engine::core::time::parse_stamp;

use super::*;

fn server() -> Server {
    let mut d = Desk::in_memory(remotecraft_engine::core::mock::demo(parse_stamp("2026-10-13T08:00:00Z").unwrap()));
    d.set_clock(parse_stamp("2026-10-13T08:00:00Z").unwrap());
    d.set_actor("riccardo");
    Server::new(d)
}

fn call(s: &mut Server, name: &str, args: Value) -> Value {
    let r = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}})).unwrap();
    assert_eq!(r["result"]["isError"], false, "{name}: {r}");
    r["result"]["structuredContent"].clone()
}

fn fails(s: &mut Server, name: &str, args: Value) -> String {
    let r = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}})).unwrap();
    assert_eq!(r["result"]["isError"], true, "{name}: {r}");
    r["result"]["content"][0]["text"].as_str().unwrap().to_string()
}

#[test]
fn handshake_tools_prompts_and_resources() {
    let mut s = server();
    let init = s.handle(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2024-11-05"}})).unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "remotecraft");
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert!(s.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
    let tools = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).unwrap();
    let list = tools["result"]["tools"].as_array().unwrap();
    assert!(list.len() >= 35);
    let mut names = std::collections::HashSet::new();
    for t in list {
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        assert!(names.insert(t["name"].as_str().unwrap().to_string()), "duplicate tool {}", t["name"]);
        for r in t["inputSchema"]["required"].as_array().unwrap() {
            assert!(t["inputSchema"]["properties"].get(r.as_str().unwrap()).is_some(), "{}: required {r} not declared", t["name"]);
        }
    }
    let prompts = s.handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "prompts/list"})).unwrap();
    for p in prompts["result"]["prompts"].as_array().unwrap() {
        let name = p["name"].as_str().unwrap();
        let r = s.handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "prompts/get", "params": {"name": name, "arguments": {"description": "x", "note": "y"}}})).unwrap();
        assert!(r["result"]["messages"][0]["content"]["text"].is_string(), "{name}");
    }
    let bad = s.handle(&json!({"jsonrpc": "2.0", "id": 4, "method": "prompts/get", "params": {"name": "log_work", "arguments": {}}})).unwrap();
    assert_eq!(bad["error"]["code"], -32602);
    for uri in [RUNNING_URI, REVIEW_URI] {
        let r = s.handle(&json!({"jsonrpc": "2.0", "id": 5, "method": "resources/read", "params": {"uri": uri}})).unwrap();
        assert!(r["result"]["contents"][0]["text"].is_string());
    }
    assert_eq!(s.handle(&json!({"jsonrpc": "2.0", "id": 6, "method": "nope"})).unwrap()["error"]["code"], -32601);
}

#[test]
fn every_tool_maps_to_a_command_and_survives_empty_args() {
    let ids = commands();
    for c in tool_commands() {
        assert!(ids.contains(&c), "tool command {c} is not an engine command");
    }
    let tools = tools_json();
    for t in tools.as_array().unwrap() {
        let name = t["name"].as_str().unwrap();
        for args in [json!({}), json!(null), json!([1]), json!({"customer": {}, "device": [], "id": "x"})] {
            let mut s = server();
            let r = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}})).unwrap();
            assert!(r["result"]["isError"].is_boolean(), "{name}: {r}");
        }
    }
    let mut s = server();
    assert!(fails(&mut s, "no_such_tool", json!({})).contains("unknown tool"));
}

#[test]
fn undeclared_arguments_are_dropped() {
    let mut s = server();
    // gdpr_export declares only `customer`: a smuggled `confirm` changes nothing, and
    // find_customers cannot be turned into another command.
    let v = call(&mut s, "find_customers", json!({"query": "corvo", "command": "gdpr.erase"}));
    assert_eq!(v["total"], 1);
}

#[test]
fn an_agent_records_a_remote_session() {
    let mut s = server();
    let who = call(&mut s, "find_device", json!({"rustdesk_id": "200 300 400"}));
    assert_eq!(who["devices"][0]["customer"]["name"], "Ferramenta Corvo");
    let started = call(&mut s, "start_intervention", json!({"device": "200 300 400", "summary": "Stampante fiscale"}));
    assert_eq!(started["connect"]["uri"], "rustdesk://connection/new/200300400");
    let stopped = call(&mut s, "stop_intervention", json!({"notes": "Driver aggiornato", "minutes": 20}));
    assert_eq!(stopped["intervention"]["minutes"], 20);
    let err = fails(&mut s, "start_intervention", json!({"device": "300400500"}));
    assert!(err.contains("consent"));
    call(
        &mut s,
        "record_consent",
        json!({"customer": "Elio Brina", "purposes": ["remote_assistance"], "signer": "Elio Brina", "evidence": "Modulo 2026/41"}),
    );
    call(&mut s, "start_intervention", json!({"device": "300400500"}));
    let report = call(&mut s, "time_report", json!({"group_by": "customer"}));
    assert!(report["rows"].as_array().unwrap().len() >= 3);
    let erased = fails(&mut s, "gdpr_erase", json!({"customer": 2, "confirm": false, "reason": "x"}));
    assert!(erased.contains("confirm"));
    let cmd = call(&mut s, "run_command", json!({"command": "app.status"}));
    assert_eq!(cmd["name"], "remotecraft");
}

#[test]
fn stdio_lines_batches_and_garbage() {
    let mut s = server();
    let input = "\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\nnot json\n[{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"},{\"jsonrpc\":\"2.0\",\"method\":\"x\"}]\n";
    let mut out = Vec::new();
    s.serve(input.as_bytes(), &mut out).unwrap();
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[1]["error"]["code"], -32700);
    assert_eq!(lines[2].as_array().unwrap().len(), 1);
}

#[test]
fn an_agent_correcting_work_into_remote_work_needs_consent_or_a_reason() {
    let mut s = server();
    let logged = call(
        &mut s,
        "log_intervention",
        json!({"customer": 3, "kind": "onsite", "summary": "Visita", "started_at": "2026-10-12 10:00", "minutes": 30}),
    );
    let id = logged["intervention"]["id"].clone();
    assert!(fails(&mut s, "update_intervention", json!({"id": id, "kind": "remote"})).contains("consent"));
    // consent_override is declared, so it reaches the engine instead of being dropped.
    let u = call(&mut s, "update_intervention", json!({"id": id, "kind": "remote", "consent_override": "Ha accettato al telefono"}));
    assert_eq!(u["intervention"]["consent_override"], "Ha accettato al telefono");
}

//! Web UI server for simulated devices.
//!
//! Serves an HTML control panel and provides WebSocket for real-time updates.

use st_comm_api::{DeviceProfile, FieldDirection, IoValue};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Start the web UI server for a simulated device.
/// Runs on `0.0.0.0:{port}` and serves an HTML page + WebSocket API.
pub async fn start_web_ui(
    device_name: String,
    profile: DeviceProfile,
    state: Arc<Mutex<HashMap<String, IoValue>>>,
    port: u16,
) {
    let listener = match TcpListener::bind(format!("0.0.0.0:{port}")).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[SIM-WEB] Failed to bind port {port}: {e}");
            return;
        }
    };
    eprintln!("[SIM-WEB] Device '{device_name}' web UI at http://localhost:{port}");

    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(s) => s,
            Err(_) => continue,
        };

        let state = Arc::clone(&state);
        let profile = profile.clone();
        let device_name = device_name.clone();

        tokio::spawn(async move {
            handle_connection(stream, &device_name, &profile, &state).await;
        });
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    device_name: &str,
    profile: &DeviceProfile,
    state: &Arc<Mutex<HashMap<String, IoValue>>>,
) {
    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf).await {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or("");

    if first_line.starts_with("GET /api/state") {
        // JSON API: return current state
        let values = state.lock().unwrap().clone();
        let json = build_state_json(profile, &values);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\n\r\n{}",
            json.len(), json
        );
        let _ = stream.write_all(response.as_bytes()).await;
    } else if first_line.starts_with("POST /api/set") {
        // JSON API: set an input value
        // Body: {"field": "DI_0", "value": true}
        let body_start = request.find("\r\n\r\n").map(|i| i + 4).unwrap_or(n);
        let body = &request[body_start..];
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
            if let (Some(field), Some(value)) = (parsed["field"].as_str(), parsed.get("value")) {
                let io_val = json_to_io_value(value);
                let mut st = state.lock().unwrap();
                // Only allow setting input fields
                let is_input = profile.fields.iter().any(|f| {
                    f.name.eq_ignore_ascii_case(field)
                        && matches!(f.direction, FieldDirection::Input | FieldDirection::Inout)
                });
                if is_input {
                    st.insert(field.to_string(), io_val);
                }
            }
        }
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nAccess-Control-Allow-Origin: *\r\n\r\nOK";
        let _ = stream.write_all(response.as_bytes()).await;
    } else if first_line.starts_with("GET / ") || first_line.starts_with("GET /index") {
        // Serve HTML page
        let html = build_html_page(device_name, profile);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
            html.len(), html
        );
        let _ = stream.write_all(response.as_bytes()).await;
    } else {
        let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found";
        let _ = stream.write_all(response.as_bytes()).await;
    }
}

fn json_to_io_value(v: &serde_json::Value) -> IoValue {
    match v {
        serde_json::Value::Bool(b) => IoValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                IoValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                IoValue::Real(f)
            } else {
                IoValue::Int(0)
            }
        }
        serde_json::Value::String(s) => IoValue::String(s.clone()),
        _ => IoValue::Int(0),
    }
}

fn io_value_to_json(v: &IoValue) -> serde_json::Value {
    match v {
        IoValue::Bool(b) => serde_json::Value::Bool(*b),
        IoValue::Int(i) => serde_json::json!(*i),
        IoValue::UInt(u) => serde_json::json!(*u),
        IoValue::Real(r) => serde_json::json!(*r),
        IoValue::String(s) => serde_json::Value::String(s.clone()),
    }
}

fn build_state_json(profile: &DeviceProfile, values: &HashMap<String, IoValue>) -> String {
    let mut fields = Vec::new();
    for field in &profile.fields {
        let value = values.get(&field.name).map(io_value_to_json)
            .unwrap_or(serde_json::Value::Null);
        fields.push(serde_json::json!({
            "name": field.name,
            "type": format!("{:?}", field.data_type),
            "direction": format!("{:?}", field.direction),
            "value": value,
        }));
    }
    serde_json::json!({ "fields": fields }).to_string()
}

fn build_html_page(device_name: &str, profile: &DeviceProfile) -> String {
    let mut inputs_html = String::new();
    let mut outputs_html = String::new();

    for field in &profile.fields {
        let is_input = matches!(field.direction, FieldDirection::Input | FieldDirection::Inout);
        let is_output = matches!(field.direction, FieldDirection::Output | FieldDirection::Inout);
        let is_bool = matches!(field.data_type, st_comm_api::FieldDataType::Bool);

        let html = if is_input {
            if is_bool {
                format!(
                    r#"<div class="field"><label>{}</label>
                    <label class="switch"><input type="checkbox" id="{}" onchange="setField('{}', this.checked)"><span class="slider"></span></label>
                    </div>"#,
                    field.name, field.name, field.name
                )
            } else {
                format!(
                    r#"<div class="field"><label>{}</label>
                    <input type="number" id="{}" value="0" onchange="setField('{}', Number(this.value))" style="width:100px">
                    </div>"#,
                    field.name, field.name, field.name
                )
            }
        } else if is_bool {
            format!(
                r#"<div class="field"><label>{}</label><span id="{}" class="led off"></span></div>"#,
                field.name, field.name
            )
        } else {
            format!(
                r#"<div class="field"><label>{}</label><span id="{}" class="value">0</span></div>"#,
                field.name, field.name
            )
        };

        if is_input && !is_output {
            inputs_html.push_str(&html);
        } else {
            outputs_html.push_str(&html);
        }
    }

    format!(r#"<!DOCTYPE html>
<html><head>
<title>{device_name} — Simulated Device</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; margin: 20px; background: #1e1e1e; color: #d4d4d4; }}
h1 {{ color: #569cd6; margin-bottom: 5px; }}
h2 {{ color: #4ec9b0; margin-top: 20px; }}
.subtitle {{ color: #808080; margin-bottom: 20px; }}
.panel {{ display: flex; gap: 40px; flex-wrap: wrap; }}
.section {{ background: #252526; border: 1px solid #3c3c3c; border-radius: 8px; padding: 20px; min-width: 300px; }}
.field {{ display: flex; align-items: center; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid #3c3c3c; }}
.field:last-child {{ border-bottom: none; }}
label {{ font-weight: 500; }}
.led {{ width: 20px; height: 20px; border-radius: 50%; display: inline-block; }}
.led.on {{ background: #4ec9b0; box-shadow: 0 0 8px #4ec9b0; }}
.led.off {{ background: #3c3c3c; }}
.value {{ font-family: 'Cascadia Code', monospace; color: #ce9178; }}
input[type=number] {{ background: #3c3c3c; color: #d4d4d4; border: 1px solid #555; padding: 4px 8px; border-radius: 4px; }}
.switch {{ position: relative; display: inline-block; width: 50px; height: 26px; }}
.switch input {{ opacity: 0; width: 0; height: 0; }}
.slider {{ position: absolute; cursor: pointer; top: 0; left: 0; right: 0; bottom: 0; background: #3c3c3c; border-radius: 26px; transition: .3s; }}
.slider:before {{ position: absolute; content: ""; height: 20px; width: 20px; left: 3px; bottom: 3px; background: #808080; border-radius: 50%; transition: .3s; }}
input:checked + .slider {{ background: #569cd6; }}
input:checked + .slider:before {{ transform: translateX(24px); background: white; }}
#status {{ margin-top: 15px; padding: 10px; background: #1a1a2e; border-radius: 4px; font-size: 12px; color: #808080; }}
</style>
</head><body>
<h1>{device_name}</h1>
<div class="subtitle">Simulated Device — {profile_name}</div>
<div class="panel">
<div class="section">
<h2>Inputs</h2>
{inputs_html}
</div>
<div class="section">
<h2>Outputs</h2>
{outputs_html}
</div>
</div>
<div id="status">Polling...</div>
<script>
function setField(name, value) {{
    fetch('/api/set', {{
        method: 'POST',
        headers: {{'Content-Type': 'application/json'}},
        body: JSON.stringify({{field: name, value: value}})
    }});
}}
function poll() {{
    fetch('/api/state').then(r => r.json()).then(data => {{
        data.fields.forEach(f => {{
            const el = document.getElementById(f.name);
            if (!el) return;
            if (f.direction === 'Output' || f.direction === 'Inout') {{
                if (f.type === 'Bool') {{
                    el.className = 'led ' + (f.value ? 'on' : 'off');
                }} else {{
                    el.textContent = f.value;
                }}
            }} else if (f.direction === 'Input') {{
                if (f.type === 'Bool' && el.type === 'checkbox') {{
                    el.checked = f.value;
                }} else if (el.type === 'number') {{
                    if (document.activeElement !== el) el.value = f.value;
                }}
            }}
        }});
        document.getElementById('status').textContent = 'Connected — ' + new Date().toLocaleTimeString();
    }}).catch(() => {{
        document.getElementById('status').textContent = 'Disconnected';
    }});
}}
setInterval(poll, 200);
poll();
</script>
</body></html>"#,
        device_name = device_name,
        profile_name = profile.name,
        inputs_html = inputs_html,
        outputs_html = outputs_html,
    )
}

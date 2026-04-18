# OPC-UA Server

The PLC runtime includes a built-in OPC-UA server that exposes all PLC
variables to HMI and SCADA clients. Any OPC-UA compliant tool — Ignition,
FUXA, WinCC OA, UaExpert, KEPServerEX, Node-RED, or custom applications —
can connect, browse variables, subscribe to live values, and write outputs.

OPC-UA is **enabled by default**. When the agent starts, the server listens
on port 4842 and is ready to accept connections immediately.

## Quick Start

### 1. Deploy your program

```bash
st-cli target install plc@192.168.1.50
st-cli bundle
curl -X POST -F "file=@MyProject.st-bundle" \
  http://192.168.1.50:4840/api/v1/program/upload
curl -X POST http://192.168.1.50:4840/api/v1/program/start
```

### 2. Connect your HMI

Point any OPC-UA client to:

```
opc.tcp://192.168.1.50:4842
```

No configuration needed. The server advertises a single endpoint with no
security and anonymous access — suitable for isolated plant networks. For
production with network exposure, see [Security Configuration](#security).

### 3. Browse and bind variables

The OPC-UA address space mirrors your PLC variables:

```
Objects
  └── PLCRuntime
       ├── Status          "Running"
       ├── Cycle Count     15432
       ├── Cycle Time (us) 12
       ├── Globals
       │    ├── io_rack_DI_0       TRUE
       │    ├── io_rack_AI_0       4095
       │    ├── pump_vfd_SPEED_REF 45.0
       │    └── pump_vfd_RUNNING   TRUE
       └── Programs
            └── Main
                 ├── counter        42
                 ├── motor_on       TRUE
                 └── pid_ctrl
                      ├── setpoint  50.0
                      └── output    47.3
```

Variables are organized into two folders:

| Folder | Contents |
|--------|----------|
| **Globals** | Flat global variables (including I/O device fields like `io_rack_DI_0`) |
| **Programs** | Program locals and function block instance fields, in a dot-separated hierarchy |

Every variable has a string NodeId in namespace 2 — the full PLC variable
name. For example:

| PLC Variable | OPC-UA NodeId |
|-------------|---------------|
| `io_rack_DI_0` | `ns=2;s=io_rack_DI_0` |
| `Main.counter` | `ns=2;s=Main.counter` |
| `Main.pid_ctrl.output` | `ns=2;s=Main.pid_ctrl.output` |

## Type Mapping

PLC data types are mapped to their natural OPC-UA equivalents:

| IEC 61131-3 | OPC-UA Type | Size |
|------------|-------------|------|
| `BOOL` | Boolean | 1 bit |
| `SINT` | SByte | 8-bit signed |
| `INT` | Int16 | 16-bit signed |
| `DINT` | Int32 | 32-bit signed |
| `LINT` | Int64 | 64-bit signed |
| `USINT` | Byte | 8-bit unsigned |
| `UINT` | UInt16 | 16-bit unsigned |
| `UDINT` | UInt32 | 32-bit unsigned |
| `ULINT` | UInt64 | 64-bit unsigned |
| `REAL` | Float | 32-bit IEEE 754 |
| `LREAL` | Double | 64-bit IEEE 754 |
| `STRING` | String | UTF-8 |
| `TIME` | Int64 | Milliseconds |
| `BYTE` | Byte | 8-bit |
| `WORD` | UInt16 | 16-bit |
| `DWORD` | UInt32 | 32-bit |
| `LWORD` | UInt64 | 64-bit |

## Reading Variables

All PLC variables are readable via OPC-UA. The server keeps node values in
sync with the PLC engine at a configurable polling interval (default 100ms).

**OPC-UA Read service:** Returns the current value immediately.

**OPC-UA Subscriptions:** Create a subscription with monitored items. The
server automatically pushes value changes at the publishing interval
requested by the client. This is the recommended approach for HMI displays
— subscribe to the variables you need, and the server sends updates only
when values change.

```
HMI Client                          OPC-UA Server              PLC Engine
    │                                     │                         │
    │  CreateSubscription(250ms)          │                         │
    │ ──────────────────────────────────► │                         │
    │                                     │                         │
    │  CreateMonitoredItems               │                         │
    │  [io_rack_DI_0, pump_vfd_SPEED_ACT] │                         │
    │ ──────────────────────────────────► │                         │
    │                                     │                         │
    │            ◄── Publish (changed values every 250ms) ──────── │
    │            ◄── Publish                                        │
    │            ◄── Publish                                        │
```

## Writing Variables

All PLC variables are writable via OPC-UA. When an HMI client writes to a
variable, the server **forces** that variable in the PLC engine — the same
mechanism used by the debug monitor's force feature.

A forced variable holds its written value regardless of what the PLC program
assigns to it. This is the correct behavior for HMI control: when an
operator sets a setpoint, the value must persist until the operator changes
it again.

```
HMI Client                    OPC-UA Server              PLC Engine
    │                               │                         │
    │  Write(pump_vfd_SPEED_REF,    │                         │
    │        45.0)                  │                         │
    │ ────────────────────────────► │                         │
    │                               │  force_variable         │
    │                               │  ("pump_vfd_SPEED_REF", │
    │                               │   "45.0")               │
    │                               │ ──────────────────────► │
    │                               │                         │
    │  StatusCode: Good             │                    (applied on
    │ ◄──────────────────────────── │                     next scan
    │                               │                     cycle)
```

**Important:** OPC-UA writes use the PLC force mechanism. A forced variable
cannot be overwritten by the PLC program until the force is removed. To
remove a force, use the HTTP API:

```bash
curl -X DELETE http://192.168.1.50:4840/api/v1/variables/force/pump_vfd_SPEED_REF
```

Or use the PLC Monitor panel in VS Code to unforce variables interactively.

## Status Nodes

The server exposes three status nodes under the `PLCRuntime` folder:

| Node | Type | Description |
|------|------|-------------|
| `_status` | String | Runtime status: `"Running"`, `"Idle"`, `"Error"`, `"DebugPaused"` |
| `_cycle_count` | UInt64 | Total scan cycles since program start |
| `_cycle_time_us` | UInt64 | Last scan cycle execution time in microseconds |

These are useful for HMI health dashboards — bind a status indicator to
`_status` and a numeric display to `_cycle_time_us`.

## Configuration

The OPC-UA server is configured in the agent's `agent.yaml` file:

```yaml
opcua_server:
  enabled: true               # default: true (OPC-UA is a core feature)
  port: 4842                  # default: 4842 (HTTP=4840, DAP=4841)
  security_policy: None       # None | Basic256Sha256 | Aes256Sha256RsaPss
  anonymous_access: true      # allow connections without credentials
  sampling_interval_ms: 100   # how often to sync values from the PLC engine
```

### Port Allocation

The agent uses three consecutive ports:

| Port | Protocol | Purpose |
|------|----------|---------|
| 4840 | HTTP | REST API — program management, variables, status |
| 4841 | TCP | DAP — VS Code remote debugging |
| 4842 | OPC-UA | HMI/SCADA variable access |

All ports are configurable. The OPC-UA port can be changed independently:

```yaml
network:
  port: 4840        # HTTP API
opcua_server:
  port: 4842        # OPC-UA (any port you want)
```

### Bind Address

By default, the OPC-UA server uses the agent's `network.bind` address. To
bind the OPC-UA server to a different interface (e.g., a dedicated HMI
network), set the `bind` field explicitly:

```yaml
network:
  bind: 127.0.0.1          # HTTP API on loopback only
opcua_server:
  bind: 192.168.10.1       # OPC-UA on the HMI network interface
```

### Sampling Interval

The `sampling_interval_ms` controls how often the OPC-UA server reads
variable values from the PLC engine. Lower values mean fresher data but
slightly higher CPU usage.

| Setting | Use Case |
|---------|----------|
| `50` | Fast HMI with real-time indicators (motion, position) |
| `100` | Default — good for most process control HMIs |
| `250` | Energy-efficient, suitable for trend displays |
| `500` | Low-bandwidth or many-variable scenarios |

The OPC-UA subscription publishing interval is independent — clients
request their own interval, and the server publishes at that rate using
the most recent sampled values.

## Certificates

The OPC-UA server **automatically generates a self-signed application
certificate** on first startup. This is required by the OPC-UA specification
— every server must have an application instance certificate, even when
using `SecurityPolicy: None`.

Certificates are stored in the PKI directory:

```
/var/lib/st-plc/retain/opcua-pki/
  own/
    cert.der           # Server's application certificate (auto-generated)
  private/
    private.pem        # Server's private key (auto-generated)
  trusted/             # Trusted client certificates
  rejected/            # Rejected client certificates (for review)
```

On first startup you'll see:

```
INFO  OPC-UA server: PKI directory = /var/lib/st-plc/retain/opcua-pki
INFO  Creating sample application instance certificate and private key
```

The certificate is persistent across restarts — it's only generated once.
If you need to regenerate it (e.g., after changing the hostname), delete
the `own/` and `private/` directories and restart the agent.

### Using a Custom Certificate

For production deployments, replace the auto-generated certificate with one
issued by your organization's CA:

1. Place your certificate at `<pki_dir>/own/cert.der` (DER format)
2. Place your private key at `<pki_dir>/private/private.pem` (PEM format)
3. Restart the agent — it will use the existing files instead of generating new ones

## Security

The default configuration uses **no security and anonymous access**, which
is appropriate for isolated plant networks where the PLC and HMI are on the
same switch and not reachable from the outside.

For networks with broader access, enable security:

```yaml
opcua_server:
  security_policy: Basic256Sha256
  message_security_mode: SignAndEncrypt
  anonymous_access: false
```

When security is enabled, message signing and encryption use the server's
application certificate (auto-generated or custom). Clients must trust
this certificate — UaExpert prompts automatically; Ignition requires manual
import into the gateway's trust store.

## Architecture

The OPC-UA server runs as a set of tokio tasks inside the agent process —
it is **not** a separate binary or service. It shares the process with the
HTTP API, WebSocket monitor, and DAP proxy.

```
┌───────────────────────────────────────────────────────────────┐
│  st-runtime process                                           │
│                                                               │
│  ┌──────────────────────┐    ┌──────────────────────────────┐ │
│  │ Engine Thread         │    │ Tokio Runtime                │ │
│  │ (dedicated OS thread) │    │                              │ │
│  │                       │    │  HTTP API       (:4840)      │ │
│  │ read_inputs()         │    │  DAP proxy      (:4841)      │ │
│  │ scan_cycle()          │    │  OPC-UA server  (:4842)      │ │
│  │ write_outputs()       │    │  WebSocket monitor           │ │
│  │ snapshot ─────────────┼───►│                              │ │
│  │                       │    │  Value sync task             │ │
│  │                ◄──────┼────│  (polls snapshot every Nms)  │ │
│  │ force_variable()      │    │                              │ │
│  └──────────────────────┘    └──────────────────────────────┘ │
└───────────────────────────────────────────────────────────────┘
```

**Thread isolation:** The PLC engine runs on its own dedicated OS thread,
completely separate from tokio. The OPC-UA server's value sync task is
I/O-bound and uses negligible CPU. Even with a free-running engine spinning
one core at maximum speed, the OPC-UA server does not interfere with scan
cycle timing.

**Data flow:**
1. The engine snapshots all variable values into `RuntimeState` (shared via
   `Arc<RwLock>`) after each scan cycle
2. The value sync task reads this snapshot every `sampling_interval_ms`
3. String values are parsed to typed OPC-UA Variants and written to the
   node manager
4. The node manager notifies active subscriptions of changed values
5. For writes: the OPC-UA server sends a `ForceVariable` command through
   the existing command channel; the engine applies it on the next scan cycle

**Catalog rebuild:** When the PLC program starts, stops, or undergoes an
online change, the variable catalog changes. The value sync task detects
this and rebuilds the OPC-UA address space — removing old nodes and
creating new ones. OPC-UA clients that had subscriptions to removed
variables receive standard `BadNodeIdUnknown` status codes.

## HMI Integration Examples

### FUXA (open-source web HMI)

1. In FUXA, go to **Devices** and add a new OPC-UA device
2. Set the endpoint URL: `opc.tcp://192.168.1.50:4842`
3. Click **Browse** to see all PLC variables
4. Drag variables onto your HMI screens
5. For writable controls (buttons, sliders), bind the write action to the
   same OPC-UA variable

### Ignition (Inductive Automation)

1. In the Ignition Gateway, go to **OPC Connections** > **Servers**
2. Add a new connection with endpoint `opc.tcp://192.168.1.50:4842`
3. In the Designer, browse the OPC-UA server under **All Providers**
4. Drag tags onto Vision or Perspective screens
5. Tags auto-update via OPC-UA subscriptions

### Node-RED

Use the `node-red-contrib-opcua` palette:

1. Add an **OpcUa-Client** node
2. Set endpoint: `opc.tcp://192.168.1.50:4842`
3. Add **OpcUa-Item** nodes for each variable
4. Set the NodeId: `ns=2;s=io_rack_DI_0`
5. Wire to a dashboard gauge, chart, or switch

### Custom Application (Python)

```python
from asyncua import Client

async def main():
    client = Client("opc.tcp://192.168.1.50:4842")
    await client.connect()

    # Read a variable
    node = client.get_node("ns=2;s=pump_vfd_SPEED_ACT")
    value = await node.read_value()
    print(f"Speed: {value} Hz")

    # Write a variable (forces it in the PLC)
    setpoint = client.get_node("ns=2;s=pump_vfd_SPEED_REF")
    await setpoint.write_value(45.0)

    # Subscribe to changes
    handler = SubHandler()
    sub = await client.create_subscription(250, handler)
    await sub.subscribe_data_change([
        client.get_node("ns=2;s=io_rack_DI_0"),
        client.get_node("ns=2;s=pump_vfd_RUNNING"),
    ])

    await client.disconnect()
```

### Custom Application (Rust)

```rust
use opcua_client::prelude::*;

let mut client = ClientBuilder::new()
    .application_name("My HMI")
    .endpoint("opc.tcp://192.168.1.50:4842", SecurityPolicy::None, MessageSecurityMode::None)
    .build()?;

let session = client.connect()?;

// Read
let node = NodeId::new(2, "pump_vfd_SPEED_ACT");
let value = session.read(&[node.into()])?;

// Write
let node = NodeId::new(2, "pump_vfd_SPEED_REF");
session.write(&[WriteValue {
    node_id: node,
    value: DataValue::new_now(Variant::Float(45.0)),
    ..Default::default()
}])?;
```

## Disabling OPC-UA

If you don't need OPC-UA (e.g., a standalone controller with no HMI):

```yaml
opcua_server:
  enabled: false
```

For size-constrained targets, you can also compile the runtime without
OPC-UA entirely:

```bash
cargo build --release --no-default-features -p st-target-agent
```

This removes the `async-opcua` dependency and produces a smaller binary.

## Troubleshooting

### OPC-UA client cannot connect

- Verify the agent is running: `curl http://192.168.1.50:4840/api/v1/health`
- Check the OPC-UA port is not blocked by a firewall
- Check agent logs: `journalctl -u st-runtime -f`
- Look for `OPC-UA server ready` in the logs

### Variables not appearing in the address space

- The address space is empty until a PLC program is started
- Start the program: `curl -X POST http://192.168.1.50:4840/api/v1/program/start`
- Refresh the client's address space (most clients cache the tree)

### Values not updating

- Check `sampling_interval_ms` in `agent.yaml` — lower it for faster updates
- Verify the PLC program is running (status should be `"Running"`)
- Check the subscription publishing interval on the client side

### Write has no effect

- OPC-UA writes use the PLC force mechanism. The value is applied on the
  next scan cycle (typically within 10ms)
- If the variable is already forced from another source (monitor panel,
  debug session), the most recent force wins
- Check agent logs for `OPC-UA: write applied` or `OPC-UA: write failed`

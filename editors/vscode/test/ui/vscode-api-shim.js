/**
 * VS Code API shim for Playwright E2E tests.
 *
 * Replaces acquireVsCodeApi() with a WebSocket bridge to the real
 * st-monitor server. Translates webview commands to WS protocol and
 * WS responses to window.postMessage events.
 *
 * The __MONITOR_PORT__ placeholder is replaced by the test server script.
 */

/* global __MONITOR_PORT__ */
var __testWs = null;
var __testWsConnected = false;
var __capturedMessages = [];

function acquireVsCodeApi() {
  return {
    postMessage: function (msg) {
      __capturedMessages.push(msg);
      if (!__testWs || __testWs.readyState !== 1) return;

      switch (msg.command) {
        case "addWatch":
          __testWs.send(
            JSON.stringify({
              method: "subscribe",
              params: { variables: [msg.variable], interval_ms: 0 },
            })
          );
          break;
        case "removeWatch":
          __testWs.send(
            JSON.stringify({
              method: "unsubscribe",
              params: { variables: [msg.variable] },
            })
          );
          break;
        case "clearWatch":
          // Can't easily unsubscribe all without knowing the list
          break;
        case "force":
          var forceValue = msg.value;
          if (forceValue === "true") forceValue = true;
          else if (forceValue === "false") forceValue = false;
          else if (/^-?\d+$/.test(forceValue))
            forceValue = parseInt(forceValue, 10);
          __testWs.send(
            JSON.stringify({
              method: "force",
              params: { variable: msg.variable, value: forceValue },
            })
          );
          break;
        case "unforce":
          __testWs.send(
            JSON.stringify({
              method: "unforce",
              params: { variable: msg.variable },
            })
          );
          break;
        case "resetStats":
          __testWs.send(JSON.stringify({ method: "resetStats" }));
          break;
      }
    },
  };
}

function __connectTestWs() {
  if (!__MONITOR_PORT__ || __MONITOR_PORT__ === 0) return;
  var url = "ws://127.0.0.1:" + __MONITOR_PORT__;
  __testWs = new WebSocket(url);

  __testWs.onopen = function () {
    __testWsConnected = true;
    document.title = "CONNECTED";
    __testWs.send(JSON.stringify({ method: "getCatalog" }));
    __testWs.send(JSON.stringify({ method: "getCycleInfo" }));
  };

  __testWs.onmessage = function (event) {
    var msg = JSON.parse(event.data);
    if (msg.type === "variableUpdate") {
      var vars = (msg.variables || []).map(function (v) {
        return { name: v.name, value: v.value, type: v.type, forced: !!v.forced };
      });
      window.postMessage(
        {
          command: "updateVariables",
          variables: vars,
          watchTree: msg.watch_tree || [],
        },
        "*"
      );
      window.postMessage(
        {
          command: "updateCycleInfo",
          info: {
            cycle_count: msg.cycle || 0,
            last_cycle_us: msg.last_cycle_us || 0,
            min_cycle_us: msg.min_cycle_us || 0,
            max_cycle_us: msg.max_cycle_us || 0,
            avg_cycle_us: msg.avg_cycle_us || 0,
            target_us: null,
            jitter_max_us: 0,
            last_period_us: 0,
          },
        },
        "*"
      );
    } else if (msg.type === "catalog") {
      window.postMessage(
        {
          command: "updateCatalog",
          catalog: (msg.variables || []).map(function (v) {
            return { name: v.name, type: v.type };
          }),
        },
        "*"
      );
    } else if (msg.type === "cycleInfo") {
      window.postMessage(
        {
          command: "updateCycleInfo",
          info: {
            cycle_count: msg.cycle_count || 0,
            last_cycle_us: msg.last_cycle_us || 0,
            min_cycle_us: msg.min_cycle_us || 0,
            max_cycle_us: msg.max_cycle_us || 0,
            avg_cycle_us: msg.avg_cycle_us || 0,
            target_us: null,
            jitter_max_us: 0,
            last_period_us: 0,
          },
        },
        "*"
      );
    }
  };

  __testWs.onclose = function () {
    __testWsConnected = false;
    document.title = "DISCONNECTED";
  };

  __testWs.onerror = function () {
    document.title = "ERROR";
  };
}

window.addEventListener("load", __connectTestWs);

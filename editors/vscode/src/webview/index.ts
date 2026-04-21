/**
 * Webview entry point for the PLC Monitor panel.
 *
 * This module is the only file that touches global state. It:
 *   1. Acquires the VS Code API.
 *   2. Reads the initial state embedded in the HTML.
 *   3. Wires up the message listener, click delegation, and sub-modules.
 *   4. Triggers the initial render.
 */

import type {
  CatalogEntry,
  HostToWebviewMessage,
  VariableValue,
  WatchNode,
  WebviewToHostMessage,
  InitialState,
} from "../shared/types";

import { fmtUs, show, validateForceValue } from "./util";
import { renderWatchTable, updateValueCellsFromTree } from "./render";
import type { AppState } from "./render";
import { setupAutocomplete } from "./autocomplete";
import {
  setupToolbar,
  populateTargets,
  updateToolbarStatus,
  renderTargetInfo,
} from "./toolbar";

// ── VS Code API ─────────────────────────────────────────────────────

declare function acquireVsCodeApi(): { postMessage(msg: unknown): void };

type VsCodeApi = { postMessage(msg: WebviewToHostMessage): void };

const vscode: VsCodeApi = acquireVsCodeApi() as VsCodeApi;

// ── Initial state from the HTML ─────────────────────────────────────

function readInitialState(): InitialState {
  const el = document.getElementById("initial-state");
  if (el && el.textContent) {
    try {
      return JSON.parse(el.textContent) as InitialState;
    } catch {
      // Fall through to defaults.
    }
  }
  return { catalog: [], watchList: [], expandedNodes: [] };
}

const initial = readInitialState();

// ── Application state ───────────────────────────────────────────────

let catalog: CatalogEntry[] = initial.catalog;
let watchList: string[] = initial.watchList;
const expandedNodes: Set<string> = new Set(initial.expandedNodes);
const valueMap = new Map<string, VariableValue>();
const typeMap = new Map<string, string>();
let serverWatchTree: WatchNode[] = [];

/** Snapshot of the current state for modules that need it. */
function getAppState(): AppState & { catalog: CatalogEntry[]; vscode: VsCodeApi } {
  return {
    watchList,
    serverWatchTree,
    expandedNodes,
    valueMap,
    typeMap,
    catalog,
    vscode,
  };
}

// ── Catalog helpers ─────────────────────────────────────────────────

function refreshCatalogDatalist(): void {
  typeMap.clear();
  for (const c of catalog) {
    typeMap.set(c.name.toLowerCase(), c.type);
  }
}

// ── Row lookup ──────────────────────────────────────────────────────

/**
 * Find a table row by its `data-var` attribute. Linear scan is fine
 * for watch lists of <100 entries and avoids CSS-escaping pitfalls.
 */
function findRow(lc: string): HTMLElement | null {
  const tbody = document.getElementById("var-body");
  if (!tbody) return null;
  const rows = tbody.children;
  for (let i = 0; i < rows.length; i++) {
    if (rows[i].getAttribute("data-var") === lc) {
      return rows[i] as HTMLElement;
    }
  }
  return null;
}

// ── Watch actions ───────────────────────────────────────────────────

function toggleNode(name: string): void {
  const lc = name.toLowerCase();
  if (expandedNodes.has(lc)) {
    expandedNodes.delete(lc);
  } else {
    expandedNodes.add(lc);
  }
  renderWatchTable(getAppState());
  vscode.postMessage({
    command: "expandedNodesChanged",
    nodes: Array.from(expandedNodes),
  });
}

function removeWatch(name: string): void {
  watchList = watchList.filter(
    (v) => v.toLowerCase() !== name.toLowerCase(),
  );
  valueMap.delete(name.toLowerCase());
  serverWatchTree = serverWatchTree.filter(
    (n) => n.fullPath.toLowerCase() !== name.toLowerCase(),
  );
  renderWatchTable(getAppState());
  vscode.postMessage({ command: "removeWatch", variable: name });
}

function forceVar(name: string): void {
  const lc = name.toLowerCase();
  const row = findRow(lc);
  const input = row ? (row.querySelector(".force-input") as HTMLInputElement | null) : null;
  const raw = input ? input.value.trim() : "";

  if (!raw) {
    if (input) input.focus();
    return;
  }

  // Validate against the declared type.
  const v = valueMap.get(lc);
  const type = (v && v.type) || typeMap.get(lc) || "";
  const canonical = validateForceValue(type, raw);

  if (canonical === null) {
    if (input) {
      input.classList.add("force-error");
      input.title = "Invalid value for type " + type;
      input.focus();
      input.select();
      setTimeout(() => {
        input.classList.remove("force-error");
        input.title = "";
      }, 1500);
    }
    return;
  }

  vscode.postMessage({ command: "force", variable: name, value: canonical });
  if (input) input.value = "";
}

function unforceVar(name: string): void {
  vscode.postMessage({ command: "unforce", variable: name });
}

function resetStats(): void {
  vscode.postMessage({ command: "resetStats" });
}

function clearAll(): void {
  if (watchList.length === 0) return;
  watchList = [];
  valueMap.clear();
  serverWatchTree = [];
  expandedNodes.clear();
  renderWatchTable(getAppState());
  vscode.postMessage({ command: "clearWatch" });
}

// ── Debug status tracking ────────────────────────────────────────────

let msgCount = 0;
let treeCount = 0;
function updateDebugStats(): void {
  const el = document.getElementById("debug-stats");
  if (el) el.textContent = `msgs:${msgCount} trees:${treeCount} wt:${serverWatchTree.length}`;
}
function setWsStatus(status: string): void {
  const el = document.getElementById("ws-status");
  if (el) el.textContent = `WS: ${status}`;
}

// ── Message handler ─────────────────────────────────────────────────

window.addEventListener("message", (event: MessageEvent) => {
  const msg = event.data as HostToWebviewMessage;
  if (!msg || !msg.command) return;
  msgCount++;

  switch (msg.command) {
    case "updateCycleInfo": {
      const ci = msg.info;
      const el = (id: string) => document.getElementById(id);
      const setText = (id: string, text: string) => {
        const e = el(id);
        if (e) e.textContent = text;
      };
      const setHtml = (id: string, html: string) => {
        const e = el(id);
        if (e) e.innerHTML = html;
      };

      setText("s-cycles", ci.cycle_count.toLocaleString());
      setHtml("s-last", fmtUs(ci.last_cycle_us));
      setHtml("s-min", fmtUs(ci.min_cycle_us));
      setHtml("s-max", fmtUs(ci.max_cycle_us));
      setHtml("s-avg", fmtUs(ci.avg_cycle_us));

      if (ci.target_us != null) {
        show("s-target-label", true);
        show("s-target", true);
        setHtml("s-target", fmtUs(ci.target_us));
        show("s-period-label", true);
        show("s-period", true);
        setHtml("s-period", fmtUs(ci.last_period_us));
        show("s-jitter-label", true);
        show("s-jitter", true);
        setHtml("s-jitter", fmtUs(ci.jitter_max_us));
      }
      break;
    }

    case "updateVariables": {
      const incomingTree = msg.watchTree || [];
      const firstRoot = incomingTree[0];
      const childInfo = firstRoot
        ? `${firstRoot.name} ch=${(firstRoot.children || []).length} k=${firstRoot.kind}`
        : "none";
      setWsStatus(`${msg.variables.length} vars, ${incomingTree.length} trees [${childInfo}]`);

      for (const v of msg.variables) {
        valueMap.set(v.name.toLowerCase(), v);
      }

      if (incomingTree.length > 0) {
        serverWatchTree = incomingTree;
        treeCount++;
      }

      // Always full render — simple and correct.
      renderWatchTable(getAppState());
      updateDebugStats();
      break;
    }

    case "resetSession":
      setWsStatus("session reset");
      valueMap.clear();
      serverWatchTree = [];
      renderWatchTable(getAppState());
      break;

    case "updateCatalog":
      catalog = msg.catalog;
      refreshCatalogDatalist();
      break;

    case "updateWatchList":
      watchList = msg.watchList;
      renderWatchTable(getAppState());
      break;

    case "updateTargetStatus":
      updateToolbarStatus(msg.status, msg.targetName);
      break;

    case "setTargets":
      populateTargets(msg.targets || [], vscode);
      break;

    case "updateTargetInfo":
      renderTargetInfo(msg.info as Record<string, unknown>);
      break;
  }
});

// ── Delegated click handler on #var-body ────────────────────────────

const varBody = document.getElementById("var-body");
if (varBody) {
  varBody.addEventListener("click", (e: Event) => {
    const target = e.target as HTMLElement;
    const btn = target.closest("[data-action]") as HTMLElement | null;
    if (!btn) return;

    const action = btn.getAttribute("data-action");
    const path = btn.getAttribute("data-path");
    if (!path) return;

    if (action === "toggle") toggleNode(path);
    else if (action === "remove") removeWatch(path);
    else if (action === "force") forceVar(path);
    else if (action === "unforce") unforceVar(path);
  });
}

// ── "Reset stats" and "Clear all" buttons ───────────────────────────

const resetBtn = document.getElementById("btn-reset-stats");
if (resetBtn) {
  resetBtn.addEventListener("click", resetStats);
}

const clearBtn = document.getElementById("btn-clear-all");
if (clearBtn) {
  clearBtn.addEventListener("click", clearAll);
}

// ── Wire sub-modules ────────────────────────────────────────────────

setupAutocomplete(getAppState, () => renderWatchTable(getAppState()));
setupToolbar(vscode);

// ── Initial render ──────────────────────────────────────────────────

refreshCatalogDatalist();
renderWatchTable(getAppState());

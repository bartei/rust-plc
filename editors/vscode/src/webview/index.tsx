/**
 * Webview entry point — Preact app for the PLC Monitor panel.
 */

import { render } from "preact";
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import type {
  CatalogEntry,
  CycleInfo,
  HostToWebviewMessage,
  InitialState,
  TargetEntry,
  VariableValue,
  WatchNode,
  WebviewToHostMessage,
} from "../shared/types";
import { WatchTable } from "./WatchTable";
import { ForceDialog } from "./ForceDialog";
import { Toolbar } from "./Toolbar";
import { CycleStats } from "./CycleStats";
import { Autocomplete } from "./Autocomplete";

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
  return { catalog: [], watchList: [], expandedNodes: [], version: "?" };
}

// ── App component ───────────────────────────────────────────────────

function App() {
  const initial = useRef(readInitialState()).current;

  // -- state --
  const [catalog, setCatalog] = useState<CatalogEntry[]>(initial.catalog);
  const [watchList, setWatchList] = useState<string[]>(initial.watchList);
  const [expandedNodes, setExpandedNodes] = useState<Set<string>>(
    () => new Set(initial.expandedNodes),
  );
  const [valueMap, setValueMap] = useState<Map<string, VariableValue>>(
    () => new Map(),
  );
  const [serverWatchTree, setServerWatchTree] = useState<WatchNode[]>([]);
  const [cycleInfo, setCycleInfo] = useState<CycleInfo | null>(null);
  const [targets, setTargets] = useState<TargetEntry[]>([]);
  const [targetStatus, setTargetStatus] = useState({ status: "offline", name: "" });
  const [targetInfo, setTargetInfo] = useState<Record<string, unknown> | null>(null);
  const [wsStatus, setWsStatus] = useState("waiting...");

  // Force dialog state
  const [forceDialog, setForceDialog] = useState<{
    variable: string;
    type: string;
    currentValue: string;
    isForced: boolean;
  } | null>(null);

  // Build a type map from catalog for quick lookup
  const typeMap = useRef(new Map<string, string>());
  useEffect(() => {
    typeMap.current.clear();
    for (const c of catalog) {
      typeMap.current.set(c.name.toLowerCase(), c.type);
    }
  }, [catalog]);

  // -- message handler --
  useEffect(() => {
    const handler = (event: MessageEvent) => {
      const msg = event.data as HostToWebviewMessage;
      if (!msg || !msg.command) return;

      switch (msg.command) {
        case "updateCycleInfo":
          setCycleInfo(msg.info);
          break;

        case "updateVariables": {
          const incomingTree = msg.watchTree || [];

          setValueMap((prev) => {
            const next = new Map(prev);
            for (const v of msg.variables) {
              next.set(v.name.toLowerCase(), v);
            }
            return next;
          });

          if (incomingTree.length > 0) {
            setServerWatchTree(incomingTree);
          }

          const firstRoot = incomingTree[0];
          const childInfo = firstRoot
            ? `${firstRoot.name} ch=${(firstRoot.children || []).length} k=${firstRoot.kind}`
            : "none";
          setWsStatus(
            `${msg.variables.length} vars, ${incomingTree.length} trees [${childInfo}]`,
          );
          break;
        }

        case "resetSession":
          setWsStatus("session reset");
          setValueMap(new Map());
          setServerWatchTree([]);
          break;

        case "updateCatalog":
          setCatalog(msg.catalog);
          break;

        case "updateWatchList":
          setWatchList(msg.watchList);
          break;

        case "updateTargetStatus":
          setTargetStatus({ status: msg.status, name: msg.targetName });
          break;

        case "setTargets":
          setTargets(msg.targets || []);
          break;

        case "updateTargetInfo":
          setTargetInfo(msg.info as Record<string, unknown>);
          break;
      }
    };

    window.addEventListener("message", handler);
    return () => window.removeEventListener("message", handler);
  }, []);

  // -- actions --
  const addWatch = useCallback((name: string) => {
    setWatchList((prev) => {
      if (prev.some((v) => v.toLowerCase() === name.toLowerCase())) return prev;
      return [...prev, name];
    });
    vscode.postMessage({ command: "addWatch", variable: name });
  }, []);

  const removeWatch = useCallback((name: string) => {
    setWatchList((prev) => prev.filter((v) => v.toLowerCase() !== name.toLowerCase()));
    setValueMap((prev) => {
      const next = new Map(prev);
      next.delete(name.toLowerCase());
      return next;
    });
    setServerWatchTree((prev) =>
      prev.filter((n) => n.fullPath.toLowerCase() !== name.toLowerCase()),
    );
    vscode.postMessage({ command: "removeWatch", variable: name });
  }, []);

  const clearAll = useCallback(() => {
    setWatchList([]);
    setValueMap(new Map());
    setServerWatchTree([]);
    setExpandedNodes(new Set());
    vscode.postMessage({ command: "clearWatch" });
  }, []);

  const toggleNode = useCallback((name: string) => {
    setExpandedNodes((prev) => {
      const next = new Set(prev);
      const lc = name.toLowerCase();
      if (next.has(lc)) next.delete(lc);
      else next.add(lc);
      vscode.postMessage({
        command: "expandedNodesChanged",
        nodes: Array.from(next),
      });
      return next;
    });
  }, []);

  const openForceDialog = useCallback(
    (variable: string) => {
      const lc = variable.toLowerCase();
      const v = valueMap.get(lc);
      const type = (v && v.type) || typeMap.current.get(lc) || "";
      const isForced = v ? !!v.forced : false;
      setForceDialog({
        variable,
        type,
        currentValue: isForced && v ? v.value : "",
        isForced,
      });
    },
    [valueMap],
  );

  const submitForce = useCallback((variable: string, value: string) => {
    vscode.postMessage({ command: "force", variable, value });
    setForceDialog(null);
  }, []);

  const submitTrigger = useCallback((variable: string, value: string) => {
    vscode.postMessage({ command: "trigger", variable, value });
    setForceDialog(null);
  }, []);

  const submitUnforce = useCallback((variable: string) => {
    vscode.postMessage({ command: "unforce", variable });
    setForceDialog(null);
  }, []);

  const unforceInline = useCallback((variable: string) => {
    vscode.postMessage({ command: "unforce", variable });
  }, []);

  const resetStats = useCallback(() => {
    vscode.postMessage({ command: "resetStats" });
  }, []);

  // -- render --
  return (
    <>
      <Toolbar
        targets={targets}
        status={targetStatus.status}
        targetName={targetStatus.name}
        targetInfo={targetInfo}
        vscode={vscode}
      />

      <h2>
        Scan Cycle
        <span class="h-actions">
          <button class="secondary" onClick={resetStats}>
            Reset Stats
          </button>
        </span>
      </h2>
      <CycleStats info={cycleInfo} />

      <h2>
        Watch List
        <span class="h-actions">
          <button class="secondary" onClick={clearAll}>
            Clear all
          </button>
        </span>
      </h2>

      <Autocomplete catalog={catalog} watchList={watchList} onAdd={addWatch} />

      <table>
        <thead>
          <tr>
            <th>Name</th>
            <th>Value</th>
            <th>Type</th>
            <th>Actions</th>
          </tr>
        </thead>
        <WatchTable
          watchList={watchList}
          serverWatchTree={serverWatchTree}
          expandedNodes={expandedNodes}
          onToggle={toggleNode}
          onRemove={removeWatch}
          onForce={openForceDialog}
          onUnforce={unforceInline}
        />
      </table>

      {forceDialog && (
        <ForceDialog
          variable={forceDialog.variable}
          type={forceDialog.type}
          currentValue={forceDialog.currentValue}
          isForced={forceDialog.isForced}
          onForce={submitForce}
          onTrigger={submitTrigger}
          onUnforce={submitUnforce}
          onClose={() => setForceDialog(null)}
        />
      )}

      <div class="status-footer">
        <span>WS: {wsStatus}</span>
        <span>PLC Monitor v{initial.version}</span>
      </div>
    </>
  );
}

// ── Mount ───────────────────────────────────────────────────────────

const root = document.getElementById("app");
if (root) render(<App />, root);

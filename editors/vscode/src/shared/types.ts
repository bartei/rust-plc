/**
 * Shared type definitions for the PLC Monitor panel.
 *
 * These interfaces are the single source of truth for message contracts
 * between the extension host (monitorPanel.ts) and the webview (webview/index.ts).
 * Both sides import from this file — type mismatches are caught at compile time.
 */

// ── Data Structures ──────────────────────────────────────────────────

/** A node in the server-built watch tree. */
export interface WatchNode {
  /** Display name (e.g. "filler", "[0]", "counter"). */
  name: string;
  /** Fully qualified path for force/unforce (e.g. "Main.filler.counter.CV"). */
  fullPath: string;
  /** Node kind: scalar, fb, struct, array, program. */
  kind: "scalar" | "fb" | "struct" | "array" | "program";
  /** Declared type (e.g. "INT", "FillController", "ARRAY[0..9] OF INT"). */
  type: string;
  /** Current value as string (leaf nodes) or empty (compound nodes). */
  value: string;
  /** Whether this variable is currently forced. */
  forced: boolean;
  /** Children for compound nodes. Empty array for leaf nodes. */
  children: WatchNode[];
}

/** A variable in the catalog (schema only, for autocomplete). */
export interface CatalogEntry {
  name: string;
  type: string;
}

/** Flat variable value (legacy, kept for backward compatibility). */
export interface VariableValue {
  name: string;
  value: string;
  type: string;
  forced: boolean;
}

/** Scan cycle statistics. */
export interface CycleInfo {
  cycle_count: number;
  last_cycle_us: number;
  min_cycle_us: number;
  max_cycle_us: number;
  avg_cycle_us: number;
  target_us: number | null;
  jitter_max_us: number;
  last_period_us: number;
}

/** Remote target entry from plc-project.yaml. */
export interface TargetEntry {
  name: string;
  host: string;
  agentPort: number;
}

// ── Extension Host → Webview Messages ────────────────────────────────

export type HostToWebviewMessage =
  | { command: "updateVariables"; variables: VariableValue[]; watchTree: WatchNode[] }
  | { command: "updateCatalog"; catalog: CatalogEntry[] }
  | { command: "updateCycleInfo"; info: CycleInfo }
  | { command: "resetSession" }
  | { command: "updateWatchList"; watchList: string[] }
  | { command: "updateTargetStatus"; status: string; targetName: string }
  | { command: "setTargets"; targets: TargetEntry[] }
  | { command: "updateTargetInfo"; info: Record<string, unknown> };

// ── Webview → Extension Host Messages ────────────────────────────────

export type WebviewToHostMessage =
  | { command: "addWatch"; variable: string }
  | { command: "removeWatch"; variable: string }
  | { command: "clearWatch" }
  | { command: "force"; variable: string; value: string }
  | { command: "trigger"; variable: string; value: string }
  | { command: "unforce"; variable: string }
  | { command: "resetStats" }
  | { command: "expandedNodesChanged"; nodes: string[] }
  | { command: "tb:install" }
  | { command: "tb:upload" }
  | { command: "tb:onlineUpdate" }
  | { command: "tb:run" }
  | { command: "tb:stop" }
  | { command: "tb:liveAttach" }
  | { command: "tb:selectTarget"; host: string; agentPort: number }
  | { command: "tb:refreshTargets" }
  | { command: "tb:fetchTargetInfo" };

// ── Initial State (passed via JSON element in HTML) ──────────────────

export interface InitialState {
  catalog: CatalogEntry[];
  watchList: string[];
  expandedNodes: string[];
  version: string;
}

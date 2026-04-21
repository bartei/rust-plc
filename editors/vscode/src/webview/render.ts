/**
 * Watch-tree rendering for the PLC Monitor webview.
 *
 * Renders from the server-provided WatchNode tree only.
 * No fallback logic — if the tree is empty, the table shows "waiting for data".
 */

import type { WatchNode, VariableValue } from "../shared/types";
import { encAttr, placeholderForType } from "./util";

// ── AppState ────────────────────────────────────────────────────────

/** Shared application state that the entry point owns and modules read. */
export interface AppState {
  watchList: string[];
  serverWatchTree: WatchNode[];
  expandedNodes: Set<string>;
  valueMap: Map<string, VariableValue>;
  typeMap: Map<string, string>;
}

// ── Single-node rendering ───────────────────────────────────────────

/**
 * Render a single WatchNode (and its children, if expanded) into HTML
 * table rows.
 *
 * Click targets use `data-action` / `data-path` attributes for
 * delegated event handling — no inline `onclick`.
 */
export function renderWatchNode(
  node: WatchNode,
  depth: number,
  isRoot: boolean,
  expandedNodes: Set<string>,
): string {
  let html = "";
  const indent = "\u00a0\u00a0\u00a0\u00a0".repeat(depth);
  const fullLc = node.fullPath.toLowerCase();
  const ea = encAttr(node.fullPath);
  const hasChildren = node.children && node.children.length > 0;
  const isExpanded = expandedNodes.has(fullLc);

  if (hasChildren) {
    const arrow = isExpanded ? "\u25BE" : "\u25B8";
    const removeBtn = isRoot
      ? ' <button class="secondary" data-action="remove" data-path="' + ea + '">Remove</button>'
      : "";

    html +=
      '<tr data-var="' + fullLc + '">' +
      '<td class="name">' + indent +
        '<span class="tree-toggle" data-action="toggle" data-path="' + ea + '">' + arrow + "</span> " +
        node.name + "</td>" +
      '<td class="value">' + (node.value || "") + "</td>" +
      '<td class="type"><i>' + (node.type || "") + "</i></td>" +
      "<td>" + removeBtn + "</td></tr>";

    if (isExpanded) {
      for (let ci = 0; ci < node.children.length; ci++) {
        html += renderWatchNode(node.children[ci], depth + 1, false, expandedNodes);
      }
    }
  } else {
    const isForced = !!node.forced;
    const placeholder = node.type ? placeholderForType(node.type) : "value";
    const removeBtn = isRoot
      ? ' <button class="secondary" data-action="remove" data-path="' + ea + '">Remove</button>'
      : "";

    html +=
      '<tr data-var="' + fullLc + '"' + (isForced ? ' class="forced"' : "") + ">" +
      '<td class="name">' + indent + (isRoot ? "" : "\u00a0\u00a0 ") + node.name + "</td>" +
      '<td class="value">' + (node.value || "") + "</td>" +
      '<td class="type"><i>' + (node.type || "") + "</i></td>" +
      "<td>" +
        '<input class="force-input" placeholder="' + placeholder + '" />' +
        ' <button data-action="force" data-path="' + ea + '">Force</button>' +
        ' <button class="secondary" data-action="unforce" data-path="' + ea + '">Unforce</button>' +
        removeBtn +
      "</td></tr>";
  }

  return html;
}

// ── Full table render ───────────────────────────────────────────────

/**
 * Render the complete watch table body from the server watch tree.
 * No fallback — the tree is the single source of truth.
 */
export function renderWatchTable(state: AppState): void {
  const tbody = document.getElementById("var-body");
  if (!tbody) return;

  if (state.watchList.length === 0) {
    tbody.innerHTML =
      '<tr><td colspan="4" class="empty-msg">' +
      "Watch list is empty. Add a variable above.</td></tr>";
    return;
  }

  // Filter tree roots to only show items in the watch list.
  const watchSet = new Set(state.watchList.map(w => w.toLowerCase()));
  const visibleRoots = state.serverWatchTree.filter(
    node => watchSet.has(node.fullPath.toLowerCase())
  );

  // Debug: show filter info in status
  const dbg = document.getElementById("debug-stats");
  if (dbg) {
    const treeNames = state.serverWatchTree.map(n => n.fullPath).join(",");
    const wl = state.watchList.join(",");
    dbg.textContent = `wl:[${wl}] tree:[${treeNames}] vis:${visibleRoots.length}`;
  }

  if (visibleRoots.length === 0) {
    tbody.innerHTML =
      '<tr><td colspan="4" class="empty-msg">' +
      "Waiting for data from PLC runtime\u2026</td></tr>";
    return;
  }

  let html = "";
  for (let si = 0; si < visibleRoots.length; si++) {
    html += renderWatchNode(visibleRoots[si], 0, true, state.expandedNodes);
  }
  tbody.innerHTML = html;
}

// ── In-place value update ───────────────────────────────────────────

/**
 * Walk the watch tree and update value/type/forced cells in-place
 * without rebuilding the table. This preserves focus on force inputs
 * and the current expand/collapse state.
 */
export function updateValueCellsFromTree(tree: WatchNode[]): void {
  const tbody = document.getElementById("var-body");
  if (!tbody) return;

  // Build a flat lookup: fullPath.toLowerCase() -> node
  const nodeMap = new Map<string, WatchNode>();
  const walk = (nodes: WatchNode[]): void => {
    for (const n of nodes) {
      nodeMap.set(n.fullPath.toLowerCase(), n);
      if (n.children) walk(n.children);
    }
  };
  walk(tree);

  const rows = tbody.children;
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i] as HTMLElement;
    const lc = row.getAttribute("data-var");
    if (!lc) continue;
    const node = nodeMap.get(lc);
    if (!node) continue;

    const valueCell = row.querySelector(".value");
    const typeCell = row.querySelector(".type");
    if (valueCell && valueCell.textContent !== node.value) {
      valueCell.textContent = node.value;
    }
    if (typeCell && node.type && typeCell.textContent !== node.type) {
      typeCell.textContent = node.type;
    }

    const wasForced = row.classList.contains("forced");
    if (wasForced !== !!node.forced) {
      row.classList.toggle("forced", !!node.forced);
    }
  }
}

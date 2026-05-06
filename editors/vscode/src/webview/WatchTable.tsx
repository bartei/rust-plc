/**
 * Watch table body — renders the variable watch tree.
 *
 * Each row is a stable Preact component keyed by fullPath, so DOM
 * elements survive value updates and button clicks always land.
 */

import type { WatchNode } from "../shared/types";
import { encAttr } from "./util";

interface WatchTableProps {
  watchList: string[];
  serverWatchTree: WatchNode[];
  expandedNodes: Set<string>;
  onToggle: (name: string) => void;
  onRemove: (name: string) => void;
  onForce: (name: string) => void;
  onUnforce: (name: string) => void;
}

export function WatchTable({
  watchList,
  serverWatchTree,
  expandedNodes,
  onToggle,
  onRemove,
  onForce,
  onUnforce,
}: WatchTableProps) {
  if (watchList.length === 0) {
    return (
      <tbody>
        <tr>
          <td colSpan={4} class="empty-msg">
            Watch list is empty. Add a variable above.
          </td>
        </tr>
      </tbody>
    );
  }

  const watchSet = new Set(watchList.map((w) => w.toLowerCase()));
  const visibleRoots = serverWatchTree.filter((node) =>
    watchSet.has(node.fullPath.toLowerCase()),
  );

  if (visibleRoots.length === 0) {
    return (
      <tbody>
        <tr>
          <td colSpan={4} class="empty-msg">
            Waiting for data from PLC runtime&hellip;
          </td>
        </tr>
      </tbody>
    );
  }

  return (
    <tbody>
      {visibleRoots.map((node) => (
        <WatchNodeRow
          key={node.fullPath}
          node={node}
          depth={0}
          isRoot={true}
          expandedNodes={expandedNodes}
          onToggle={onToggle}
          onRemove={onRemove}
          onForce={onForce}
          onUnforce={onUnforce}
        />
      ))}
    </tbody>
  );
}

// ── Single node row ─────────────────────────────────────────────────

interface WatchNodeRowProps {
  node: WatchNode;
  depth: number;
  isRoot: boolean;
  expandedNodes: Set<string>;
  onToggle: (name: string) => void;
  onRemove: (name: string) => void;
  onForce: (name: string) => void;
  onUnforce: (name: string) => void;
}

function WatchNodeRow({
  node,
  depth,
  isRoot,
  expandedNodes,
  onToggle,
  onRemove,
  onForce,
  onUnforce,
}: WatchNodeRowProps) {
  const indent = "\u00a0\u00a0\u00a0\u00a0".repeat(depth);
  const hasChildren = node.children && node.children.length > 0;
  const isExpanded = expandedNodes.has(node.fullPath.toLowerCase());

  if (hasChildren) {
    const arrow = isExpanded ? "\u25BE" : "\u25B8";
    return (
      <>
        <tr key={node.fullPath} data-var={node.fullPath.toLowerCase()}>
          <td class="name">
            {indent}
            <span
              class="tree-toggle"
              onClick={() => onToggle(node.fullPath)}
            >
              {arrow}
            </span>{" "}
            {node.name}
            <RetainBadge node={node} />
          </td>
          <td class="value">{node.value || ""}</td>
          <td class="type">
            <i>{node.type || ""}</i>
          </td>
          <td>
            {isRoot && (
              <button
                class="secondary"
                onClick={() => onRemove(node.fullPath)}
              >
                Remove
              </button>
            )}
          </td>
        </tr>
        {isExpanded &&
          node.children.map((child) => (
            <WatchNodeRow
              key={child.fullPath}
              node={child}
              depth={depth + 1}
              isRoot={false}
              expandedNodes={expandedNodes}
              onToggle={onToggle}
              onRemove={onRemove}
              onForce={onForce}
              onUnforce={onUnforce}
            />
          ))}
      </>
    );
  }

  // Leaf node
  const isForced = !!node.forced;
  return (
    <tr
      key={node.fullPath}
      data-var={node.fullPath.toLowerCase()}
      class={isForced ? "forced" : undefined}
    >
      <td class="name">
        {indent}
        {isRoot ? "" : "\u00a0\u00a0 "}
        {node.name}
        <RetainBadge node={node} />
      </td>
      <td class="value">{node.value || ""}</td>
      <td class="type">
        <i>{node.type || ""}</i>
      </td>
      <td>
        {isForced && (
          <span class="forced-value">{node.value || ""}</span>
        )}
        <button onClick={() => onForce(node.fullPath)}>Force</button>
        {isForced ? (
          <>{" "}
            <button
              class="secondary"
              onClick={() => onUnforce(node.fullPath)}
            >
              Unforce
            </button>
          </>
        ) : (
          <>{" "}
            <button
              class="secondary"
              onClick={() => onForce(node.fullPath)}
            >
              Trigger
            </button>
          </>
        )}
        {isRoot && (
          <>{" "}
            <button
              class="secondary"
              onClick={() => onRemove(node.fullPath)}
            >
              Remove
            </button>
          </>
        )}
      </td>
    </tr>
  );
}

// ── Retain / Persistent badge ───────────────────────────────────────

interface RetainBadgeProps {
  node: WatchNode;
}

/**
 * Small inline badge for IEC RETAIN / PERSISTENT variables.
 * Renders nothing for ordinary variables.
 *  - "R"  : RETAIN (survives warm restart)
 *  - "P"  : PERSISTENT (survives cold restart)
 *  - "RP" : RETAIN PERSISTENT
 */
function RetainBadge({ node }: RetainBadgeProps) {
  const r = !!node.retain;
  const p = !!node.persistent;
  if (!r && !p) {
    return null;
  }
  const label = r && p ? "RP" : r ? "R" : "P";
  const title = r && p
    ? "RETAIN PERSISTENT — survives warm and cold restart"
    : r
      ? "RETAIN — survives warm restart"
      : "PERSISTENT — survives cold restart";
  return (
    <>
      {" "}
      <span
        class="retain-badge"
        data-retain={r ? "1" : "0"}
        data-persistent={p ? "1" : "0"}
        title={title}
      >
        {label}
      </span>
    </>
  );
}

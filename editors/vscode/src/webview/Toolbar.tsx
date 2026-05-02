/**
 * Deployment toolbar: target selection, status indicator, action buttons.
 */

import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import type { TargetEntry, WebviewToHostMessage } from "../shared/types";
import { fmtUptime } from "./util";

type VsCodeApi = { postMessage(msg: WebviewToHostMessage): void };

interface ToolbarProps {
  targets: TargetEntry[];
  status: string;
  targetName: string;
  targetInfo: Record<string, unknown> | null;
  vscode: VsCodeApi;
}

export function Toolbar({
  targets,
  status,
  targetName,
  targetInfo,
  vscode,
}: ToolbarProps) {
  const [showInfo, setShowInfo] = useState(false);
  const selectRef = useRef<HTMLSelectElement>(null);

  // Auto-select first target when targets change
  const prevTargetsLen = useRef(0);
  useEffect(() => {
    if (targets.length === 1 && prevTargetsLen.current !== 1 && selectRef.current) {
      selectRef.current.value = targets[0].host;
      vscode.postMessage({
        command: "tb:selectTarget",
        host: targets[0].host,
        agentPort: targets[0].agentPort,
      });
    }
    prevTargetsLen.current = targets.length;
  }, [targets, vscode]);

  const send = useCallback(
    (command: WebviewToHostMessage["command"]) => {
      vscode.postMessage({ command } as WebviewToHostMessage);
    },
    [vscode],
  );

  const handleTargetChange = useCallback(
    (e: Event) => {
      const select = e.target as HTMLSelectElement;
      const opt = select.options[select.selectedIndex];
      const host = opt.dataset.host || "";
      const port = parseInt(opt.dataset.port || "4840", 10);
      vscode.postMessage({
        command: "tb:selectTarget",
        host: host || "",
        agentPort: host ? port : 0,
      });
    },
    [vscode],
  );

  const handleFetchInfo = useCallback(() => {
    setShowInfo(true);
    vscode.postMessage({ command: "tb:fetchTargetInfo" } as WebviewToHostMessage);
  }, [vscode]);

  // Dot class
  let dotClass = "tb-dot offline";
  if (status === "running") dotClass = "tb-dot running";
  else if (status === "idle" || status === "stopped") dotClass = "tb-dot stopped";
  else if (status === "error") dotClass = "tb-dot error";

  // Status text
  let statusText = targetName || "No target";
  if (status === "running") statusText = (targetName ? targetName + " \u2014 " : "") + "Running";
  else if (status === "idle" || status === "stopped")
    statusText = (targetName ? targetName + " \u2014 " : "") + "Stopped";
  else if (status === "error")
    statusText = (targetName ? targetName + " \u2014 " : "") + "Error";

  const isRunning = status === "running";

  return (
    <>
      <div class="deploy-toolbar">
        <div class="tb-group">
          <button onClick={() => send("tb:install")} title="Install or upgrade the PLC runtime on the target">
            <span class="tb-icon">&#x2B07;</span> Install
          </button>
        </div>
        <div class="tb-sep" />
        <div class="tb-group">
          <button onClick={() => send("tb:upload")} title="Upload PLC program to target">
            <span class="tb-icon">&#x2191;</span> Upload
          </button>
          <button onClick={() => send("tb:onlineUpdate")} title="Online update — hot-reload without stopping">
            <span class="tb-icon">&#x21BB;</span> Online
          </button>
        </div>
        <div class="tb-sep" />
        <div class="tb-group">
          <button onClick={() => send("tb:run")} disabled={isRunning} title="Start or restart the PLC program">
            <span class="tb-icon">&#x25B6;</span> Run
          </button>
          <button onClick={() => send("tb:stop")} disabled={!isRunning} title="Stop the PLC program">
            <span class="tb-icon">&#x25A0;</span> Stop
          </button>
          <button
            onClick={() => send("tb:liveAttach")}
            disabled={!isRunning}
            title="Attach the VS Code debugger to the running program — execution continues, breakpoints fire on demand"
          >
            <span class="tb-icon">&#x1F41E;</span> Live Attach
          </button>
        </div>
        <div class="tb-status">
          <span class={dotClass} />
          <select
            ref={selectRef}
            id="tb-target-select"
            title="Select deployment target"
            onChange={handleTargetChange}
          >
            <option value="">-- No target --</option>
            {targets.map((t) => (
              <option
                key={t.host}
                value={t.host}
                data-host={t.host}
                data-port={String(t.agentPort)}
              >
                {t.name} ({t.host}:{t.agentPort})
              </option>
            ))}
          </select>
          <button
            onClick={() => send("tb:refreshTargets")}
            title="Reload targets from plc-project.yaml"
            style="background:none;border:none;color:var(--vscode-descriptionForeground);cursor:pointer;font-size:12px;padding:0 2px;"
          >
            &#x21BB;
          </button>
          <button onClick={handleFetchInfo} title="Fetch target information" style="font-size:11px;padding:3px 8px;">
            &#x2139; Info
          </button>
          <span>{statusText}</span>
        </div>
      </div>

      {showInfo && (
        <TargetInfoPanel info={targetInfo} onClose={() => setShowInfo(false)} />
      )}
    </>
  );
}

// ── Target info panel ───────────────────────────────────────────────

function TargetInfoPanel({
  info,
  onClose,
}: {
  info: Record<string, unknown> | null;
  onClose: () => void;
}) {
  let content;
  if (!info) {
    content = <span>Fetching target information...</span>;
  } else if (info.error) {
    content = (
      <div class="ti-row">
        <span class="ti-value ti-err">{String(info.error)}</span>
      </div>
    );
  } else {
    const agent = info.agent as Record<string, unknown> | undefined;
    const program = info.program as Record<string, unknown> | undefined;
    content = (
      <>
        {agent && (
          <>
            <div class="ti-row">
              <span class="ti-label">Agent:</span>
              <span class="ti-value ti-ok">
                {String(agent.agent_name)} v{String(agent.agent_version)}
              </span>
            </div>
            <div class="ti-row">
              <span class="ti-label">Platform:</span>
              <span class="ti-value">
                {String(agent.os)} / {String(agent.arch)}
              </span>
            </div>
            <div class="ti-row">
              <span class="ti-label">Uptime:</span>
              <span class="ti-value">{fmtUptime(Number(agent.uptime_secs))}</span>
            </div>
          </>
        )}
        {program ? (
          <>
            <div class="ti-row">
              <span class="ti-label">Program:</span>
              <span class="ti-value">
                {String(program.name)} v{String(program.version)}
              </span>
            </div>
            <div class="ti-row">
              <span class="ti-label">Mode:</span>
              <span class="ti-value">{String(program.mode)}</span>
            </div>
            <div class="ti-row">
              <span class="ti-label">Deployed:</span>
              <span class="ti-value">{String(program.deployed_at)}</span>
            </div>
          </>
        ) : (
          agent && (
            <div class="ti-row">
              <span class="ti-label">Program:</span>
              <span class="ti-value ti-warn">No program deployed</span>
            </div>
          )
        )}
        {info.status && (
          <div class="ti-row">
            <span class="ti-label">Status:</span>
            <span
              class={`ti-value ${info.status === "running" ? "ti-ok" : info.status === "error" ? "ti-err" : ""}`}
            >
              {String(info.status)}
            </span>
          </div>
        )}
      </>
    );
  }

  return (
    <div class="target-info visible">
      <div class="ti-title">
        <span>Target Information</span>
        <button class="ti-close" onClick={onClose}>
          &#x2715;
        </button>
      </div>
      <div>{content}</div>
    </div>
  );
}

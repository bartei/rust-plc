/**
 * Deployment toolbar: target selection, status indicator, and action buttons.
 */

import type { TargetEntry, WebviewToHostMessage } from "../shared/types";
import { fmtUptime } from "./util";

/** Minimal VS Code API surface needed by the toolbar. */
export type VsCodeApi = { postMessage(msg: WebviewToHostMessage): void };

// ── Toolbar button wiring ───────────────────────────────────────────

/**
 * Attach click handlers to the static toolbar buttons.
 *
 * These buttons exist in the HTML from the start, so we can wire them
 * once during initialisation.
 */
export function setupToolbar(vscode: VsCodeApi): void {
  const wire = (id: string, command: WebviewToHostMessage["command"]): void => {
    const btn = document.getElementById(id);
    if (btn) {
      btn.addEventListener("click", () => {
        vscode.postMessage({ command } as WebviewToHostMessage);
      });
    }
  };

  wire("tb-install", "tb:install");
  wire("tb-upload", "tb:upload");
  wire("tb-online-update", "tb:onlineUpdate");
  wire("tb-run", "tb:run");
  wire("tb-stop", "tb:stop");
  wire("tb-refresh-targets", "tb:refreshTargets");

  // "Fetch target info" button opens the info panel.
  const infoBtn = document.getElementById("tb-fetch-info");
  if (infoBtn) {
    infoBtn.addEventListener("click", () => {
      const panel = document.getElementById("target-info-panel");
      const content = document.getElementById("target-info-content");
      if (panel) panel.classList.add("visible");
      if (content) content.innerHTML = "Fetching target information...";
      vscode.postMessage({ command: "tb:fetchTargetInfo" } as WebviewToHostMessage);
    });
  }

  // Close button inside the target-info panel.
  const closeBtn = document.getElementById("tb-close-info");
  if (closeBtn) {
    closeBtn.addEventListener("click", () => {
      const panel = document.getElementById("target-info-panel");
      if (panel) panel.classList.remove("visible");
    });
  }

  // Target dropdown change handler.
  const select = document.getElementById("tb-target-select") as HTMLSelectElement | null;
  if (select) {
    select.addEventListener("change", () => {
      const opt = select.options[select.selectedIndex];
      const host = opt.dataset.host || "";
      const port = parseInt(opt.dataset.port || "4840", 10);

      if (host) {
        vscode.postMessage({ command: "tb:selectTarget", host, agentPort: port });
      } else {
        vscode.postMessage({ command: "tb:selectTarget", host: "", agentPort: 0 });
      }

      // Reset status indicator when switching targets.
      updateToolbarStatus(
        "offline",
        host ? opt.textContent || "" : "",
      );
    });
  }
}

// ── Target dropdown population ──────────────────────────────────────

/**
 * Populate the `#tb-target-select` dropdown from extension-provided
 * target entries and auto-select when only one target exists.
 */
export function populateTargets(targets: TargetEntry[], vscode: VsCodeApi): void {
  const select = document.getElementById("tb-target-select") as HTMLSelectElement | null;
  if (!select) return;

  // Preserve current selection if possible.
  const current = select.value;
  select.innerHTML = '<option value="">-- No target --</option>';

  for (const t of targets) {
    const opt = document.createElement("option");
    opt.value = t.host;
    opt.textContent = t.name + " (" + t.host + ":" + t.agentPort + ")";
    opt.dataset.host = t.host;
    opt.dataset.port = String(t.agentPort);
    select.appendChild(opt);
  }

  // Restore previous selection, or auto-select if only one target.
  if (current && [...select.options].some((o) => o.value === current)) {
    select.value = current;
  } else if (targets.length === 1) {
    select.value = targets[0].host;
  }

  // Notify extension so it starts polling the selected target.
  if (select.value) {
    const opt = select.options[select.selectedIndex];
    const host = opt.dataset.host || "";
    const port = parseInt(opt.dataset.port || "4840", 10);
    if (host) {
      vscode.postMessage({ command: "tb:selectTarget", host, agentPort: port });
      updateToolbarStatus("offline", opt.textContent || "");
    }
  }
}

// ── Status indicator ────────────────────────────────────────────────

/** Update the dot + text status indicator in the toolbar. */
export function updateToolbarStatus(status: string, targetName: string): void {
  const dot = document.getElementById("tb-dot");
  const text = document.getElementById("tb-status-text");
  if (!dot || !text) return;

  dot.className = "tb-dot";
  if (status === "running") {
    dot.classList.add("running");
    text.textContent = targetName ? targetName + " \u2014 Running" : "Running";
  } else if (status === "idle" || status === "stopped") {
    dot.classList.add("stopped");
    text.textContent = targetName ? targetName + " \u2014 Stopped" : "Stopped";
  } else if (status === "error") {
    dot.classList.add("error");
    text.textContent = targetName ? targetName + " \u2014 Error" : "Error";
  } else {
    dot.classList.add("offline");
    text.textContent = targetName || "No target";
  }

  // Enable/disable Run/Stop buttons based on state.
  const runBtn = document.getElementById("tb-run") as HTMLButtonElement | null;
  const stopBtn = document.getElementById("tb-stop") as HTMLButtonElement | null;
  if (runBtn) runBtn.disabled = status === "running";
  if (stopBtn) stopBtn.disabled = status !== "running";
}

// ── Target info panel ───────────────────────────────────────────────

/** Render target information into the info panel. */
export function renderTargetInfo(info: Record<string, unknown>): void {
  const panel = document.getElementById("target-info-panel");
  const content = document.getElementById("target-info-content");
  if (!panel || !content) return;
  panel.classList.add("visible");

  if (info.error) {
    content.innerHTML =
      '<div class="ti-row"><span class="ti-value ti-err">' +
      String(info.error) +
      "</span></div>";
    return;
  }

  let html = "";

  // Agent info
  const agent = info.agent as Record<string, unknown> | undefined;
  if (agent) {
    html +=
      '<div class="ti-row"><span class="ti-label">Agent:</span>' +
      '<span class="ti-value ti-ok">' +
      String(agent.agent_name) +
      " v" +
      String(agent.agent_version) +
      "</span></div>";
    html +=
      '<div class="ti-row"><span class="ti-label">Platform:</span>' +
      '<span class="ti-value">' +
      String(agent.os) +
      " / " +
      String(agent.arch) +
      "</span></div>";
    html +=
      '<div class="ti-row"><span class="ti-label">Uptime:</span>' +
      '<span class="ti-value">' +
      fmtUptime(Number(agent.uptime_secs)) +
      "</span></div>";
  }

  // Program info
  const program = info.program as Record<string, unknown> | undefined;
  if (program) {
    html +=
      '<div class="ti-row"><span class="ti-label">Program:</span>' +
      '<span class="ti-value">' +
      String(program.name) +
      " v" +
      String(program.version) +
      "</span></div>";
    html +=
      '<div class="ti-row"><span class="ti-label">Mode:</span>' +
      '<span class="ti-value">' +
      String(program.mode) +
      "</span></div>";
    html +=
      '<div class="ti-row"><span class="ti-label">Deployed:</span>' +
      '<span class="ti-value">' +
      String(program.deployed_at) +
      "</span></div>";
  } else if (agent) {
    html +=
      '<div class="ti-row"><span class="ti-label">Program:</span>' +
      '<span class="ti-value ti-warn">No program deployed</span></div>';
  }

  // Runtime status
  if (info.status) {
    const cls =
      info.status === "running"
        ? "ti-ok"
        : info.status === "error"
          ? "ti-err"
          : "";
    html +=
      '<div class="ti-row"><span class="ti-label">Status:</span>' +
      '<span class="ti-value ' +
      cls +
      '">' +
      String(info.status) +
      "</span></div>";
  }

  content.innerHTML =
    html || '<span class="ti-value ti-warn">No information available</span>';
}

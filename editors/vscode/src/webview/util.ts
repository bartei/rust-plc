/**
 * Pure utility functions shared across webview modules.
 *
 * These have zero DOM dependencies (except `show()`) and zero imports
 * from other webview modules, so they can be unit-tested trivially.
 */

/** Format a microsecond duration into a human-readable string. */
export function fmtUs(us: number): string {
  if (us >= 1000) {
    return (us / 1000).toFixed(us >= 10000 ? 0 : 1) + " ms";
  }
  return us + " \u00b5s";
}

/** Encode a string for safe use inside an HTML double-quoted attribute. */
export function encAttr(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");
}

/** Return a sensible placeholder string for a force-value input. */
export function placeholderForType(type: string): string {
  const t = type.toUpperCase();
  if (t === "BOOL") return "TRUE / FALSE";
  if (t === "STRING" || t === "WSTRING") return "text";
  if (t === "REAL" || t === "LREAL") return "1.5";
  return "0";
}

/**
 * Validate a user-entered force value against the variable's declared type.
 *
 * Returns the canonicalized value to send to the backend, or `null` if the
 * input is invalid for this type.
 *
 * - BOOL accepts true/false/0/1 (case insensitive).
 * - Integer types accept signed decimals; range is not enforced here
 *   because the DAP/VM clamps to the declared type at load time.
 * - Float types accept decimals or integers.
 * - STRING accepts any non-empty input.
 */
export function validateForceValue(type: string, raw: string): string | null {
  if (!raw) return null;
  const t = (type || "").toUpperCase();

  if (t === "BOOL") {
    const lower = raw.toLowerCase();
    if (lower === "true" || lower === "1") return "true";
    if (lower === "false" || lower === "0") return "false";
    return null;
  }

  const intTypes = [
    "SINT", "USINT", "BYTE", "INT", "UINT", "WORD",
    "DINT", "UDINT", "DWORD", "LINT", "ULINT", "LWORD",
  ];
  if (intTypes.indexOf(t) !== -1) {
    if (!/^-?\d+$/.test(raw)) return null;
    return raw;
  }

  if (t === "REAL" || t === "LREAL") {
    if (!/^-?\d+(\.\d+)?([eE][+-]?\d+)?$/.test(raw)) return null;
    return raw;
  }

  if (t === "STRING" || t === "WSTRING") {
    return raw;
  }

  // Unknown / complex type -- accept as-is and let the backend reject.
  return raw;
}

/** Format an uptime in seconds into a compact "Xh Ym" string. */
export function fmtUptime(secs: number): string {
  if (secs < 60) return secs + "s";
  if (secs < 3600) return Math.floor(secs / 60) + "m " + (secs % 60) + "s";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return h + "h " + m + "m";
}

/** Show or hide a DOM element by id. */
export function show(id: string, visible: boolean): void {
  const el = document.getElementById(id);
  if (el) {
    el.style.display = visible ? "" : "none";
  }
}

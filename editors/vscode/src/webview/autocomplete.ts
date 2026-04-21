/**
 * Catalog autocomplete for the "Add variable" input.
 *
 * Provides a dropdown with fuzzy-filtered catalog entries, keyboard
 * navigation (arrow keys, Enter, Escape), and mouse selection.
 */

import type { CatalogEntry } from "../shared/types";
import type { WebviewToHostMessage } from "../shared/types";
import type { AppState } from "./render";

/** The extended state slice that autocomplete needs. */
export interface AutocompleteState extends AppState {
  catalog: CatalogEntry[];
  vscode: { postMessage(msg: WebviewToHostMessage): void };
}

/**
 * Wire up the autocomplete dropdown on `#add-input`.
 *
 * @param getState  Returns the current application state (catalog,
 *                  watchList, vscode API, etc.). Called on every
 *                  interaction so it always reflects the latest data.
 */
let onAddCallback: (() => void) | null = null;

export function setupAutocomplete(
  getState: () => AutocompleteState,
  onAdd?: () => void,
): void {
  onAddCallback = onAdd || null;
  const input = document.getElementById("add-input") as HTMLInputElement | null;
  const dd = document.getElementById("autocomplete-dropdown");
  if (!input || !dd) return;

  // Wire the Add button
  const addBtn = document.getElementById("btn-add");
  if (addBtn) {
    addBtn.addEventListener("click", () => addFromInput());
  }

  let selectedIdx = -1;

  // ── helpers ───────────────────────────────────────────────────

  function addFromInput(): void {
    const state = getState();
    const name = input!.value.trim();
    if (!name) return;

    if (!state.watchList.some((v) => v.toLowerCase() === name.toLowerCase())) {
      state.watchList.push(name);
    }
    // Always send addWatch (triggers WS subscribe + fresh data push)
    state.vscode.postMessage({ command: "addWatch", variable: name });
    input!.value = "";
    dd!.classList.remove("visible");
    // Notify caller to re-render
    if (onAddCallback) onAddCallback();
  }

  function showDropdown(): void {
    const state = getState();
    const query = input!.value.trim().toLowerCase();

    if (!query || state.catalog.length === 0) {
      dd!.classList.remove("visible");
      return;
    }

    const matches = state.catalog.filter((c) =>
      c.name.toLowerCase().includes(query),
    );
    if (matches.length === 0) {
      dd!.classList.remove("visible");
      return;
    }

    selectedIdx = -1;
    dd!.innerHTML = matches
      .slice(0, 50)
      .map(
        (c, i) =>
          '<div class="autocomplete-item" data-name="' +
          c.name +
          '" data-idx="' +
          i +
          '">' +
          "<span>" +
          c.name +
          "</span>" +
          '<span class="item-type">' +
          c.type +
          "</span>" +
          "</div>",
      )
      .join("");

    dd!.classList.add("visible");

    dd!.querySelectorAll(".autocomplete-item").forEach((item) => {
      item.addEventListener("mousedown", (e: Event) => {
        e.preventDefault();
        input!.value = (item as HTMLElement).getAttribute("data-name") || "";
        dd!.classList.remove("visible");
        addFromInput();
      });
    });
  }

  // ── event wiring ──────────────────────────────────────────────

  input.addEventListener("input", showDropdown);
  input.addEventListener("focus", showDropdown);
  input.addEventListener("blur", () => {
    setTimeout(() => {
      dd.classList.remove("visible");
    }, 150);
  });

  input.addEventListener("keydown", (e: KeyboardEvent) => {
    const items = dd.querySelectorAll(".autocomplete-item");

    if (e.key === "ArrowDown" && dd.classList.contains("visible")) {
      e.preventDefault();
      selectedIdx = Math.min(selectedIdx + 1, items.length - 1);
      items.forEach((el, i) =>
        el.classList.toggle("selected", i === selectedIdx),
      );
      if (items[selectedIdx]) {
        items[selectedIdx].scrollIntoView({ block: "nearest" });
      }
    } else if (e.key === "ArrowUp" && dd.classList.contains("visible")) {
      e.preventDefault();
      selectedIdx = Math.max(selectedIdx - 1, 0);
      items.forEach((el, i) =>
        el.classList.toggle("selected", i === selectedIdx),
      );
      if (items[selectedIdx]) {
        items[selectedIdx].scrollIntoView({ block: "nearest" });
      }
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (selectedIdx >= 0 && selectedIdx < items.length) {
        input.value =
          (items[selectedIdx] as HTMLElement).getAttribute("data-name") || "";
        dd.classList.remove("visible");
      }
      addFromInput();
    } else if (e.key === "Escape") {
      dd.classList.remove("visible");
    }
  });
}

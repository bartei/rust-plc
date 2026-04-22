/**
 * Catalog autocomplete for the "Add variable" input.
 */

import { useCallback, useRef, useState } from "preact/hooks";
import type { CatalogEntry } from "../shared/types";

interface AutocompleteProps {
  catalog: CatalogEntry[];
  watchList: string[];
  onAdd: (name: string) => void;
}

export function Autocomplete({ catalog, watchList, onAdd }: AutocompleteProps) {
  const [query, setQuery] = useState("");
  const [showDropdown, setShowDropdown] = useState(false);
  const [selectedIdx, setSelectedIdx] = useState(-1);
  const inputRef = useRef<HTMLInputElement>(null);

  const matches =
    query.trim().length > 0
      ? catalog
          .filter((c) => c.name.toLowerCase().includes(query.trim().toLowerCase()))
          .slice(0, 50)
      : [];

  const addFromInput = useCallback(() => {
    const name = inputRef.current?.value.trim() || "";
    if (!name) return;
    onAdd(name);
    setQuery("");
    setShowDropdown(false);
    if (inputRef.current) inputRef.current.value = "";
  }, [onAdd]);

  const selectItem = useCallback(
    (name: string) => {
      if (inputRef.current) inputRef.current.value = name;
      setQuery(name);
      setShowDropdown(false);
      onAdd(name);
      setQuery("");
      if (inputRef.current) inputRef.current.value = "";
    },
    [onAdd],
  );

  const handleInput = useCallback((e: Event) => {
    const val = (e.target as HTMLInputElement).value;
    setQuery(val);
    setSelectedIdx(-1);
    setShowDropdown(val.trim().length > 0);
  }, []);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === "ArrowDown" && showDropdown) {
        e.preventDefault();
        setSelectedIdx((prev) => Math.min(prev + 1, matches.length - 1));
      } else if (e.key === "ArrowUp" && showDropdown) {
        e.preventDefault();
        setSelectedIdx((prev) => Math.max(prev - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (selectedIdx >= 0 && selectedIdx < matches.length) {
          if (inputRef.current) inputRef.current.value = matches[selectedIdx].name;
        }
        addFromInput();
      } else if (e.key === "Escape") {
        setShowDropdown(false);
      }
    },
    [showDropdown, matches, selectedIdx, addFromInput],
  );

  return (
    <div class="add-row">
      <input
        ref={inputRef}
        placeholder="Add variable to watch (start typing for suggestions)..."
        autocomplete="off"
        onInput={handleInput}
        onFocus={() => {
          if (query.trim().length > 0) setShowDropdown(true);
        }}
        onBlur={() => {
          // Delay to allow mousedown on dropdown items
          setTimeout(() => setShowDropdown(false), 150);
        }}
        onKeyDown={handleKeyDown}
      />
      {showDropdown && matches.length > 0 && (
        <div class="autocomplete-dropdown visible">
          {matches.map((c, i) => (
            <div
              key={c.name}
              class={`autocomplete-item${i === selectedIdx ? " selected" : ""}`}
              onMouseDown={(e) => {
                e.preventDefault();
                selectItem(c.name);
              }}
            >
              <span>{c.name}</span>
              <span class="item-type">{c.type}</span>
            </div>
          ))}
        </div>
      )}
      <button onClick={addFromInput}>Add</button>
    </div>
  );
}

/**
 * Force/Trigger/Unforce dialog — modal popup for variable forcing.
 */

import { useEffect, useRef, useState } from "preact/hooks";
import { placeholderForType, validateForceValue } from "./util";

interface ForceDialogProps {
  variable: string;
  type: string;
  currentValue: string;
  isForced: boolean;
  onForce: (variable: string, value: string) => void;
  onTrigger: (variable: string, value: string) => void;
  onUnforce: (variable: string) => void;
  onClose: () => void;
}

export function ForceDialog({
  variable,
  type,
  currentValue,
  isForced,
  onForce,
  onTrigger,
  onUnforce,
  onClose,
}: ForceDialogProps) {
  const [inputValue, setInputValue] = useState(currentValue);
  const [error, setError] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  function validate(): string | null {
    const raw = inputValue.trim();
    if (!raw) {
      setError("Please enter a value.");
      inputRef.current?.focus();
      return null;
    }
    const canonical = validateForceValue(type, raw);
    if (canonical === null) {
      setError(`Invalid value for type ${type}`);
      inputRef.current?.focus();
      inputRef.current?.select();
      return null;
    }
    return canonical;
  }

  function handleForce() {
    const val = validate();
    if (val !== null) onForce(variable, val);
  }

  function handleTrigger() {
    const val = validate();
    if (val !== null) onTrigger(variable, val);
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      handleForce();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }

  function handleOverlayClick(e: MouseEvent) {
    if ((e.target as HTMLElement).classList.contains("force-dialog-overlay")) {
      onClose();
    }
  }

  return (
    <div class="force-dialog-overlay visible" onClick={handleOverlayClick}>
      <div class="force-dialog">
        <div class="force-dialog-title">Force Variable</div>
        <div class="force-dialog-var">{variable}</div>
        <div class="force-dialog-type">{type || "unknown"}</div>
        <input
          ref={inputRef}
          class="force-dialog-input"
          placeholder={type ? placeholderForType(type) : "value"}
          value={inputValue}
          onInput={(e) => {
            setInputValue((e.target as HTMLInputElement).value);
            setError("");
          }}
          onKeyDown={handleKeyDown}
          autocomplete="off"
        />
        <div class="force-dialog-error">{error}</div>
        <div class="force-dialog-buttons">
          <button onClick={handleForce}>Force</button>
          <button class="secondary" onClick={handleTrigger}>
            Trigger (1 cycle)
          </button>
          {isForced && (
            <button class="secondary" onClick={() => onUnforce(variable)}>
              Unforce
            </button>
          )}
          <button class="secondary" onClick={onClose}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

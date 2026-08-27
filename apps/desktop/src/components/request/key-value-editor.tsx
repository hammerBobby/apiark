import type { KeyboardEvent } from "react";
import type { KeyValuePair } from "@apiark/types";
import { Plus, Trash2 } from "lucide-react";

let kvCounter = 0;
const kvId = () => `kv_${Date.now()}_${++kvCounter}`;

interface KeyValueEditorProps {
  pairs: KeyValuePair[];
  onChange: (pairs: KeyValuePair[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
}

export function KeyValueEditor({
  pairs,
  onChange,
  keyPlaceholder = "Key",
  valuePlaceholder = "Value",
}: KeyValueEditorProps) {
  const update = (index: number, field: keyof KeyValuePair, value: string | boolean) => {
    const updated = pairs.map((p, i) =>
      i === index ? { ...p, [field]: value } : p,
    );
    onChange(updated);
  };

  const addRow = () => {
    onChange([...pairs, { id: kvId(), key: "", value: "", enabled: true }]);
  };

  const removeRow = (index: number) => {
    if (pairs.length <= 1) {
      onChange([{ id: kvId(), key: "", value: "", enabled: true }]);
      return;
    }
    onChange(pairs.filter((_, i) => i !== index));
  };

  /**
   * Pressing Tab on a value input should move to the next row's key input,
   * adding a new empty row when already on the last row — instead of jumping
   * to the row's delete button.
   */
  const focusKeyById = (rowId: string) => {
    // Wait for React to flush the (possibly newly added) row before focusing.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const el = document.querySelector<HTMLInputElement>(
          `input[data-kv-id="${CSS.escape(rowId)}"]`,
        );
        el?.focus();
        el?.select();
      });
    });
  };

  const handleValueTab = (index: number, e: KeyboardEvent) => {
    if (e.key !== "Tab") return;
    e.preventDefault();
    let targetId: string;
    if (index === pairs.length - 1) {
      // On the last row: append a new row and focus its key input.
      targetId = kvId();
      onChange([...pairs, { id: targetId, key: "", value: "", enabled: true }]);
    } else {
      // Otherwise: move to the next row's key input.
      targetId = pairs[index + 1].id;
    }
    focusKeyById(targetId);
  };

  return (
    <div className="space-y-1">
      {/* Header row */}
      <div className="grid grid-cols-[auto_1fr_1fr_auto] items-center gap-2 px-1 text-xs text-[var(--color-text-muted)]">
        <span className="w-5" />
        <span>{keyPlaceholder}</span>
        <span>{valuePlaceholder}</span>
        <span className="w-7" />
      </div>

      {/* Rows */}
      {pairs.map((pair, index) => (
        <div
          key={pair.id}
          className="grid grid-cols-[auto_1fr_1fr_auto] items-center gap-2 px-1"
        >
          <input
            type="checkbox"
            checked={pair.enabled}
            onChange={(e) => update(index, "enabled", e.target.checked)}
            className="h-4 w-4 accent-blue-500"
          />
          <input
            type="text"
            data-kv-id={pair.id}
            value={pair.key}
            onChange={(e) => update(index, "key", e.target.value)}
            placeholder={keyPlaceholder}
            className="rounded bg-[var(--color-elevated)] px-2 py-1 text-sm text-[var(--color-text-primary)] placeholder-[var(--color-text-dimmed)] outline-none focus:ring-1 focus:ring-blue-500"
          />
          <input
            type="text"
            data-kv-id={pair.id}
            value={pair.value}
            onChange={(e) => update(index, "value", e.target.value)}
            onKeyDown={(e) => handleValueTab(index, e)}
            placeholder={valuePlaceholder}
            className="rounded bg-[var(--color-elevated)] px-2 py-1 text-sm text-[var(--color-text-primary)] placeholder-[var(--color-text-dimmed)] outline-none focus:ring-1 focus:ring-blue-500"
          />
          <button
            onClick={() => removeRow(index)}
            className="rounded p-1 text-[var(--color-text-muted)] hover:bg-[var(--color-border)] hover:text-red-400"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        </div>
      ))}

      {/* Add row button */}
      <button
        onClick={addRow}
        className="flex items-center gap-1 px-1 pt-1 text-xs text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]"
      >
        <Plus className="h-3 w-3" /> Add
      </button>
    </div>
  );
}

import { useState, useRef, useMemo, useEffect, type KeyboardEvent } from "react";
import type { KeyValuePair } from "@apiark/types";
import { Plus, Trash2 } from "lucide-react";

let kvCounter = 0;
const kvId = () => `kv_${Date.now()}_${++kvCounter}`;

/**
 * A curated list of common request headers for the auto-complete dropdown.
 * Deliberately excludes hop-by-hop headers (`Connection`, `Keep-Alive`,
 * `Transfer-Encoding`…), pseudo-headers (`:path`, `:authority`) and browser-/
 * proxy-specific headers (`sec-ch-ua-*`), which are meaningless — and often
 * rejected — by the reqwest client.
 */
export const COMMON_HEADERS = [
  "Authorization",
  "Content-Type",
  "Accept",
  "Accept-Language",
  "Accept-Encoding",
  "Cache-Control",
  "User-Agent",
  "Referer",
  "Origin",
  "Cookie",
  "If-Modified-Since",
  "If-None-Match",
  "If-Match",
  "Range",
  "ETag",
  "Pragma",
  "Expires",
  "Last-Modified",
  "Location",
  "X-Requested-With",
  "X-API-Key",
  "X-Request-Id",
  "X-Correlation-Id",
  "X-CSRF-Token",
  "X-Trace-Id",
];

interface KeyValueEditorProps {
  pairs: KeyValuePair[];
  onChange: (pairs: KeyValuePair[]) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
  /** Optional suggestions for the key column (e.g. COMMON_HEADERS). */
  keySuggestions?: string[];
}

export function KeyValueEditor({
  pairs,
  onChange,
  keyPlaceholder = "Key",
  valuePlaceholder = "Value",
  keySuggestions,
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
          <KeyInput
            id={pair.id}
            value={pair.key}
            suggestions={keySuggestions ?? []}
            otherKeys={pairs
              .filter((_, i) => i !== index)
              .map((p) => p.key.toLowerCase())}
            placeholder={keyPlaceholder}
            onChange={(v) => update(index, "key", v)}
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

/**
 * Text input for the key column with an optional auto-complete dropdown.
 *
 * Behaviour:
 *  - Shows a filtered list when focused or typing (an empty field reveals all).
 *  - Filters case-insensitively by substring and hides keys already used by
 *    other rows (so duplicates aren't suggested).
 *  - Only fills the key name when an item is chosen — never the value, never
 *    auto-appends headers, never rewrites what the user typed.
 */
function KeyInput({
  id,
  value,
  suggestions,
  otherKeys,
  placeholder,
  onChange,
}: {
  id?: string;
  value: string;
  suggestions: string[];
  otherKeys: string[];
  placeholder: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const wrapRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const filtered = useMemo(() => {
    if (!suggestions.length) return [];
    const q = value.trim().toLowerCase();
    return suggestions.filter(
      (s) =>
        !otherKeys.includes(s.toLowerCase()) &&
        (q === "" || s.toLowerCase().includes(q)),
    );
  }, [suggestions, value, otherKeys]);

  useEffect(() => {
    setActive(0);
  }, [value, open]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const choose = (s: string) => {
    onChange(s);
    setOpen(false);
    inputRef.current?.focus();
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (!open || filtered.length === 0) {
      // Let the user open the list with the down arrow even before typing.
      if (e.key === "ArrowDown" && suggestions.length > 0) {
        e.preventDefault();
        setOpen(true);
      }
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(filtered[active]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
    }
  };

  const show = open && filtered.length > 0;

  return (
    <div className="relative" ref={wrapRef}>
      <input
        ref={inputRef}
        type="text"
        data-kv-id={id}
        value={value}
        placeholder={placeholder}
        onFocus={() => setOpen(true)}
        onChange={(e) => {
          onChange(e.target.value);
          setOpen(true);
        }}
        onKeyDown={onKeyDown}
        className="w-full rounded bg-[var(--color-elevated)] px-2 py-1 text-sm text-[var(--color-text-primary)] placeholder-[var(--color-text-dimmed)] outline-none focus:ring-1 focus:ring-blue-500"
      />
      {show && (
        <ul className="absolute left-0 right-0 z-30 mt-1 max-h-56 overflow-auto rounded border border-[var(--color-border)] bg-[var(--color-surface)] py-1 shadow-lg">
          {filtered.map((s, i) => (
            <li
              key={s}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(s);
              }}
              onMouseEnter={() => setActive(i)}
              className={`cursor-pointer px-2 py-1 text-sm ${
                i === active
                  ? "bg-[var(--color-accent)]/15 text-[var(--color-text-primary)]"
                  : "text-[var(--color-text-secondary)]"
              }`}
            >
              {s}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

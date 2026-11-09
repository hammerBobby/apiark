import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  AuthConfig,
  HistoryEntry,
  HistoryRequestSnapshot,
  HttpMethod,
  KeyValuePair,
  RequestBody,
  ResponseData,
  TabProtocol,
} from "@apiark/types";
import { useHistoryStore } from "@/stores/history-store";
import { useTabStore } from "@/stores/tab-store";
import { Search, Trash2 } from "lucide-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { HistorySkeleton } from "@/components/ui/skeleton";
import { EmptyState, ClockEmptyIcon } from "@/components/ui/empty-state";

const METHOD_COLORS: Record<string, string> = {
  GET: "text-green-500",
  POST: "text-yellow-500",
  PUT: "text-blue-500",
  PATCH: "text-purple-500",
  DELETE: "text-red-500",
  HEAD: "text-cyan-500",
  OPTIONS: "text-gray-500",
};

function statusColor(status?: number): string {
  if (!status) return "text-[var(--color-text-muted)]";
  if (status < 300) return "text-green-500";
  if (status < 400) return "text-yellow-500";
  return "text-red-500";
}

function timeAgo(timestamp: string): string {
  const diff = Date.now() - new Date(timestamp).getTime();
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

const ROW_HEIGHT = 32;

const emptyRow = (): KeyValuePair => ({
  id: crypto.randomUUID(),
  key: "",
  value: "",
  enabled: true,
});

/** Normalize key/value rows persisted in history (may be missing ids/flags). */
function toRows(value: unknown, trailingEmptyRow = false): KeyValuePair[] {
  const rows: KeyValuePair[] = Array.isArray(value)
    ? value
        .filter(
          (row): row is KeyValuePair =>
            !!row &&
            typeof row === "object" &&
            typeof (row as KeyValuePair).key === "string" &&
            typeof (row as KeyValuePair).value === "string",
        )
        .map((row) => ({
          id: row.id || crypto.randomUUID(),
          key: row.key,
          value: row.value,
          enabled: row.enabled !== false,
          valueType: row.valueType === "file" ? "file" : undefined,
        }))
    : [];
  const last = rows[rows.length - 1];
  if (trailingEmptyRow && (!last || last.key.trim() || last.value.trim())) {
    rows.push(emptyRow());
  }
  return rows;
}

/** Legacy entries only stored the URL, so recover query params from it. */
function paramsFromUrl(url: string): KeyValuePair[] {
  const qIndex = url.indexOf("?");
  if (qIndex === -1) return toRows([], true);
  const rows: KeyValuePair[] = [];
  for (const [key, value] of new URLSearchParams(url.slice(qIndex + 1))) {
    rows.push({ id: crypto.randomUUID(), key, value, enabled: true });
  }
  return toRows(rows, true);
}

function normalizeBody(value: unknown): RequestBody {
  if (!value || typeof value !== "object") {
    return { type: "none", content: "", formData: [] };
  }
  const body = value as Partial<RequestBody>;
  return {
    type: (body.type ?? "none") as RequestBody["type"],
    content: typeof body.content === "string" ? body.content : "",
    formData: Array.isArray(body.formData)
      ? body.formData.map((row) => ({ ...row, id: row.id || crypto.randomUUID() }))
      : [],
  };
}

function parseHistoryRequest(raw: string): HistoryRequestSnapshot | null {
  try {
    const value: unknown = JSON.parse(raw);
    return value && typeof value === "object" ? (value as HistoryRequestSnapshot) : null;
  } catch {
    return null;
  }
}

function parseHistoryResponse(raw: string | null | undefined): ResponseData | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as Partial<ResponseData>;
    if (!value || typeof value !== "object" || typeof value.status !== "number") return null;
    return {
      status: value.status,
      statusText: value.statusText ?? "",
      headers: toRows(value.headers),
      cookies: Array.isArray(value.cookies) ? value.cookies : [],
      body: typeof value.body === "string" ? value.body : "",
      timeMs: value.timeMs ?? 0,
      sizeBytes: value.sizeBytes ?? 0,
      truncated: value.truncated,
      fullSize: value.fullSize,
    };
  } catch {
    return null;
  }
}

export function HistoryPanel() {
  const { t } = useTranslation();
  const { entries, loading, loadHistory, searchHistory, clearHistory } =
    useHistoryStore();
  const [searchQuery, setSearchQuery] = useState("");
  const parentRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    loadHistory();
  }, [loadHistory]);

  const handleSearch = (query: string) => {
    setSearchQuery(query);
    if (query.trim()) {
      searchHistory(query);
    } else {
      loadHistory();
    }
  };

  const handleRestore = (entry: HistoryEntry) => {
    const snapshot = parseHistoryRequest(entry.requestJson);
    const response = parseHistoryResponse(entry.responseJson);
    const protocol: TabProtocol = snapshot?.protocol === "graphql" ? "graphql" : "http";
    const graphql = snapshot?.graphql
      ? {
          query: snapshot.graphql.query ?? "",
          variables: snapshot.graphql.variables ?? "{}",
          operationName: snapshot.graphql.operationName ?? "",
          schemaJson: null,
        }
      : null;

    useTabStore.getState().newTab();
    const tabId = useTabStore.getState().activeTabId;
    if (!tabId) return;

    // Restore the whole request (headers, params, body, scripts) and the saved
    // response, not just method + URL.
    useTabStore.setState((state) => ({
      tabs: state.tabs.map((tab) =>
        tab.id === tabId
          ? {
              ...tab,
              name: entry.requestName || "History Request",
              protocol,
              graphql,
              method: (snapshot?.method ?? entry.method) as HttpMethod,
              url: snapshot?.url || entry.url,
              headers: toRows(snapshot?.headers, true),
              params: snapshot?.params?.length
                ? toRows(snapshot.params, true)
                : paramsFromUrl(snapshot?.url || entry.url),
              pathVariables: snapshot?.pathVariables ?? {},
              body: normalizeBody(snapshot?.body),
              auth: (snapshot?.auth ?? { type: "none" }) as AuthConfig,
              preRequestScript: snapshot?.preRequestScript ?? null,
              postResponseScript: snapshot?.postResponseScript ?? null,
              testScript: snapshot?.testScript ?? null,
              assertions: snapshot?.assertions ?? null,
              response,
              error: null,
              loading: false,
              testResults: [],
              assertionResults: [],
              consoleOutput: [],
              isDirty: false,
              conflictState: null,
              undoStack: [],
              redoStack: [],
            }
          : tab,
      ),
    }));
  };

  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 10,
  });

  return (
    <div className="px-2">
      {/* Search + clear */}
      <div className="mb-1 flex items-center gap-1">
        <div className="flex flex-1 items-center rounded bg-[var(--color-elevated)] px-2">
          <Search className="h-3 w-3 text-[var(--color-text-dimmed)]" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => handleSearch(e.target.value)}
            placeholder={t("history.search")}
            className="w-full bg-transparent px-1.5 py-1 text-xs text-[var(--color-text-primary)] placeholder-[var(--color-text-dimmed)] outline-none"
          />
        </div>
        {entries.length > 0 && (
          <button
            onClick={clearHistory}
            className="rounded p-1 text-[var(--color-text-dimmed)] hover:bg-[var(--color-border)] hover:text-red-400"
            title={t("history.clear")}
          >
            <Trash2 className="h-3 w-3" />
          </button>
        )}
      </div>

      {/* Entries */}
      {loading ? (
        <HistorySkeleton />
      ) : entries.length === 0 ? (
        searchQuery ? (
          <p className="py-2 text-center text-xs text-[var(--color-text-dimmed)]">{t("history.noResults")}</p>
        ) : (
          <EmptyState
            icon={<ClockEmptyIcon size={32} />}
            title={t("history.noHistory")}
            description={t("history.noHistory")}
          />
        )
      ) : (
        <div
          ref={parentRef}
          className="max-h-[300px] overflow-y-auto"
        >
          <div
            style={{
              height: `${virtualizer.getTotalSize()}px`,
              width: "100%",
              position: "relative",
            }}
          >
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const entry = entries[virtualRow.index];
              return (
                <button
                  key={entry.id}
                  onClick={() => handleRestore(entry)}
                  className="flex w-full items-center gap-1.5 rounded px-2 text-left hover:bg-[var(--color-elevated)]"
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  <span
                    className={`w-9 shrink-0 text-[10px] font-bold ${METHOD_COLORS[entry.method] ?? "text-[var(--color-text-muted)]"}`}
                  >
                    {entry.method}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-xs text-[var(--color-text-secondary)]">
                    {entry.url}
                  </span>
                  <span className={`shrink-0 text-[10px] ${statusColor(entry.status ?? undefined)}`}>
                    {entry.status ?? "—"}
                  </span>
                  <span className="shrink-0 text-[10px] text-[var(--color-text-dimmed)]">
                    {timeAgo(entry.timestamp)}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

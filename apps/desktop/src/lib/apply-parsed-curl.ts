import type { HttpMethod, KeyValuePair, BodyType, AuthConfig, RequestBody } from "@apiark/types";
import type { ParsedCurlRequest } from "@apiark/types";
import { useTabStore } from "@/stores/tab-store";

let kvCounter = 0;
const kvId = () => `kv_${Date.now()}_${++kvCounter}`;

/** Result of {@link applyParsedCurlToCurrentTab} — used for callers (toast, telemetry, etc.). */
export interface ApplyCurlResult {
  method: HttpMethod;
  url: string;
}

/**
 * Apply a parsed cURL request to the currently active tab.
 *
 * Unlike {@link importCurlIntoNewTab}, this mutates the existing tab so the
 * user's URL/method/headers are replaced atomically. It uses tab-store
 * actions so the rest of the UI (params table sync, body tabs, etc.)
 * re-renders correctly.
 *
 * The function has no side effects beyond updating the store — no toast or
 * dialog. Callers handle UX.
 */
export function applyParsedCurlToCurrentTab(parsed: ParsedCurlRequest): ApplyCurlResult {
  const { setMethod, setUrl, setHeaders, setParams, setBody, setAuth } = useTabStore.getState();

  const method = parsed.method.toUpperCase() as HttpMethod;
  setMethod(method);

  // setUrl already syncs query params from the URL into the params table,
  // but it always appends a trailing empty row for UX. After a cURL import we
  // want a *clean* table (just the params we recognised), so we strip the
  // trailing empty row afterwards by overwriting with only populated entries.
  setUrl(parsed.url);
  const existingParams = useTabStore.getState().tabs.find(
    (t) => t.id === useTabStore.getState().activeTabId,
  )?.params ?? [];
  const populatedParams = existingParams.filter((p) => p.key.trim().length > 0);
  setParams(populatedParams);

  const headers: KeyValuePair[] = Object.entries(parsed.headers).map(([key, value]) => ({
    id: kvId(),
    key,
    value,
    enabled: true,
  }));
  if (headers.length > 0) {
    // Keep one empty row at the bottom for UX, mirroring CurlImportDialog behaviour.
    setHeaders([...headers, { id: kvId(), key: "", value: "", enabled: true }]);
  }

  const formData = (parsed.formData ?? []).map((f) => ({
    id: kvId(),
    key: f.key,
    value: f.value,
    enabled: true,
    valueType: f.valueType === "file" ? ("file" as const) : ("text" as const),
  }));

  if (parsed.body || formData.length > 0) {
    const type: BodyType = formData.length > 0
      ? "form-data"
      : ((parsed.bodyType as BodyType) ?? "raw");
    const body: RequestBody = {
      type,
      content: parsed.body ?? "",
      formData,
    };
    setBody(body);
  }

  if (parsed.authBasic) {
    const auth: AuthConfig = {
      type: "basic",
      username: parsed.authBasic[0],
      password: parsed.authBasic[1],
    };
    setAuth(auth);
  }

  return { method, url: parsed.url };
}

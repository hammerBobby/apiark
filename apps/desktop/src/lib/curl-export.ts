import { readRequestFile } from "@/lib/tauri-api";
import { generateCurl } from "@/lib/code-generators";
import { requestFileToTab } from "@/stores/tab-store";

/**
 * Load a request file from disk, build an in-memory Tab, and serialize it to a
 * cURL command string. Used by "Copy as cURL" actions that don't have an
 * already-open tab (e.g. from the collection tree hover/context menu).
 */
export async function buildCurlFromFile(
  filePath: string,
  collectionPath: string | null,
): Promise<string> {
  const file = await readRequestFile(filePath);
  const tab = requestFileToTab(file, filePath, collectionPath);
  return generateCurl(tab);
}

/**
 * Copy a string to the system clipboard. Returns true on success.
 * Falls back to a temporary textarea + execCommand for environments where the
 * async clipboard API is unavailable (e.g. some webview contexts).
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // fall through to legacy path
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}

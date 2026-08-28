import type { Tab } from "@apiark/types";

/** Clean up a GraphQL JSON body: collapse \r\n and consecutive whitespace in the query string */
function cleanGraphQLBody(body: string): string {
  try {
    const parsed = JSON.parse(body);
    if (typeof parsed.query === "string") {
      parsed.query = parsed.query
        .replace(/\r\n/g, " ")
        .replace(/\n/g, " ")
        .replace(/\s{2,}/g, " ")
        .trim();
      return JSON.stringify(parsed, null, 2);
    }
  } catch {
    // not valid JSON, return as-is
  }
  return body;
}

export function generateCurl(tab: Tab): string {
  const parts: string[] = [];
  // Chrome DevTools "Copy as cURL" style: `curl --url '<URL>'`
  // URL already contains query params (synced from Params tab)
  parts.push(`curl --url '${tab.url}'`);

  const headers: string[] = [];
  for (const h of tab.headers) {
    if (h.key.trim() && h.enabled) {
      // Chrome emits `-H 'Key: Value'`; empty-value headers become `-H 'Key;'`
      const key = h.key.trim();
      headers.push(
        h.value.trim() === "" ? `-H '${key};'` : `-H '${key}: ${h.value}'`,
      );
    }
  }

  if (tab.auth.type === "bearer") {
    headers.push(`-H 'Authorization: Bearer ${tab.auth.token}'`);
  } else if (tab.auth.type === "api-key" && tab.auth.addTo === "header") {
    headers.push(`-H '${tab.auth.key}: ${tab.auth.value}'`);
  }

  // Method: GET/HEAD are the default, so like Chrome we only emit --request for others.
  if (tab.method !== "GET" && tab.method !== "HEAD") {
    parts.push(`--request ${tab.method}`);
  }

  for (const h of headers) parts.push(h);

  if (tab.auth.type === "basic") {
    parts.push(`-u '${tab.auth.username}:${tab.auth.password}'`);
  }

  const body = tab.body;
  if (body.type === "form-data") {
    // multipart/form-data: emit one -F per field. File fields become @path entries.
    for (const fd of body.formData) {
      if (!fd.enabled || !fd.key.trim()) continue;
      const isFile = fd.valueType === "file";
      const escapedValue = fd.value
        .replace(/\\/g, "\\\\")
        .replace(/'/g, "'\\''");
      parts.push(
        isFile
          ? `-F '${fd.key}=@${escapedValue}'`
          : `-F '${fd.key}=${escapedValue}'`,
      );
    }
  } else if (body.type === "urlencoded") {
    for (const kv of body.formData) {
      if (!kv.enabled || !kv.key.trim()) continue;
      const encoded = encodeURIComponent(kv.key) + "=" + encodeURIComponent(kv.value);
      parts.push(`--data-urlencode '${encoded.replace(/'/g, "'\\''")}'`);
    }
  } else if (body.type === "binary") {
    if (body.content.trim()) {
      const escaped = body.content.replace(/'/g, "'\\''");
      parts.push(`--data-binary '@${escaped}'`);
    }
  } else if (body.type !== "none" && body.content.trim()) {
    const content = body.type === "json" ? cleanGraphQLBody(body.content) : body.content;
    const escaped = content.replace(/'/g, "'\\''");
    // Chrome uses --data-raw for arbitrary text bodies.
    parts.push(`--data-raw '${escaped}'`);
    if (body.type === "json") {
      parts.push(`-H 'Content-Type: application/json'`);
    } else if (body.type === "xml") {
      parts.push(`-H 'Content-Type: application/xml'`);
    } else if (body.type === "raw" && tab.headers.every((h) => h.key.toLowerCase() !== "content-type")) {
      // only add a default content-type when the user hasn't already set one
      parts.push(`-H 'Content-Type: text/plain'`);
    }
  }

  return parts.join(" \\\n  ");
}

export function generateJsFetch(tab: Tab): string {
  // URL already contains query params (synced from Params tab)
  const url = tab.url;

  const headers: Record<string, string> = {};
  for (const h of tab.headers) {
    if (h.key.trim() && h.enabled) {
      headers[h.key] = h.value;
    }
  }

  if (tab.auth.type === "bearer") {
    headers["Authorization"] = `Bearer ${tab.auth.token}`;
  } else if (tab.auth.type === "basic") {
    headers["Authorization"] = `Basic ${btoa(`${tab.auth.username}:${tab.auth.password}`)}`;
  } else if (tab.auth.type === "api-key" && tab.auth.addTo === "header") {
    headers[tab.auth.key] = tab.auth.value;
  }

  const hasHeaders = Object.keys(headers).length > 0;
  const hasBody = tab.body.type !== "none" && tab.body.content.trim();

  const options: string[] = [];
  options.push(`  method: "${tab.method}",`);

  if (hasHeaders) {
    options.push(`  headers: ${JSON.stringify(headers, null, 4).replace(/\n/g, "\n  ")},`);
  }

  if (hasBody) {
    if (tab.body.type === "json") {
      const content = cleanGraphQLBody(tab.body.content).trim();
      options.push(`  body: JSON.stringify(${content}),`);
    } else {
      options.push(`  body: ${JSON.stringify(tab.body.content)},`);
    }
  }

  const lines = [
    `const response = await fetch("${url}", {`,
    ...options,
    `});`,
    ``,
    `const data = await response.json();`,
    `console.log(data);`,
  ];

  return lines.join("\n");
}

export function generatePythonRequests(tab: Tab): string {
  const headers: Record<string, string> = {};
  for (const h of tab.headers) {
    if (h.key.trim() && h.enabled) {
      headers[h.key] = h.value;
    }
  }

  if (tab.auth.type === "bearer") {
    headers["Authorization"] = `Bearer ${tab.auth.token}`;
  } else if (tab.auth.type === "api-key" && tab.auth.addTo === "header") {
    headers[tab.auth.key] = tab.auth.value;
  }

  const lines = ["import requests", ""];

  // URL already contains query params (synced from Params tab)
  lines.push(`url = "${tab.url}"`);

  if (Object.keys(headers).length > 0) {
    lines.push(`headers = ${pythonDict(headers)}`);
  }

  const hasBody = tab.body.type !== "none" && tab.body.content.trim();
  if (hasBody) {
    if (tab.body.type === "json") {
      const content = cleanGraphQLBody(tab.body.content).trim();
      lines.push(`json_data = ${content}`);
    } else {
      lines.push(`data = ${JSON.stringify(tab.body.content)}`);
    }
  }

  // Build the request call
  const args = [`url`];
  if (Object.keys(headers).length > 0) args.push("headers=headers");
  if (hasBody) {
    args.push(tab.body.type === "json" ? "json=json_data" : "data=data");
  }
  if (tab.auth.type === "basic") {
    args.push(`auth=("${tab.auth.username}", "${tab.auth.password}")`);
  }

  lines.push("");
  lines.push(`response = requests.${tab.method.toLowerCase()}(${args.join(", ")})`);
  lines.push(`print(response.status_code)`);
  lines.push(`print(response.json())`);

  return lines.join("\n");
}

function pythonDict(obj: Record<string, string>): string {
  const entries = Object.entries(obj).map(([k, v]) => `    "${k}": "${v}"`);
  return `{\n${entries.join(",\n")}\n}`;
}

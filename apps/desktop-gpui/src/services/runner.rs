//! Collection runner — executes every request in a collection, optionally
//! driven by a CSV data file.

use std::collections::HashMap;
use std::path::Path;

use crate::models::{CollectionNode, KeyValuePair, RunnerResult, Tab};
use crate::services::{collection, http::SendParams, script, HttpService};

pub fn load_data_rows(path: Option<&Path>) -> Result<Vec<HashMap<String, String>>, String> {
    let Some(path) = path else {
        return Ok(vec![HashMap::new()]);
    };
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(file);
    let headers = reader.headers().map_err(|e| e.to_string())?.clone();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| e.to_string())?;
        let row = headers
            .iter()
            .zip(record.iter())
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        rows.push(row);
    }
    if rows.is_empty() {
        rows.push(HashMap::new());
    }
    Ok(rows)
}

fn collect_requests(node: &CollectionNode, out: &mut Vec<(String, String)>) {
    match node {
        CollectionNode::Request { name, path, .. } => out.push((name.clone(), path.clone())),
        CollectionNode::Case { name, path, .. } => out.push((name.clone(), path.clone())),
        _ => {}
    }
    for child in node.children() {
        collect_requests(child, out);
    }
}

fn interpolate(text: &str, variables: &HashMap<String, String>) -> String {
    let mut out = text.to_string();
    for (key, value) in variables {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    out
}

pub async fn run_collection(
    collection_path: &str,
    variables: &HashMap<String, String>,
    data_rows: Vec<HashMap<String, String>>,
    timeout_ms: u64,
    verify_ssl: bool,
    follow_redirects: bool,
    proxy_url: String,
) -> Result<Vec<RunnerResult>, String> {
    let tree = collection::load_collection_tree(Path::new(collection_path))?;
    let mut requests = Vec::new();
    collect_requests(&tree, &mut requests);
    if requests.is_empty() {
        return Err("Collection has no requests to run".into());
    }

    let mut results = Vec::new();
    for (row_index, row) in data_rows.iter().enumerate() {
        for (name, path) in &requests {
            let file = match collection::read_request(Path::new(path)) {
                Ok(file) => file,
                Err(e) => {
                    results.push(RunnerResult {
                        name: name.clone(),
                        method: String::new(),
                        url: path.clone(),
                        status: None,
                        time_ms: 0,
                        passed: false,
                        error: Some(e),
                    });
                    continue;
                }
            };
            let mut tab = Tab::from_request_file(path, collection_path, &file);
            let mut vars = variables.clone();
            vars.extend(row.clone());

            let current_headers: HashMap<String, String> = tab
                .headers
                .iter()
                .filter(|pair| pair.enabled && !pair.key.is_empty())
                .map(|pair| (pair.key.clone(), pair.value.clone()))
                .collect();
            if let Some(script_source) = tab
                .pre_request_script
                .as_deref()
                .filter(|script| !script.trim().is_empty())
            {
                match script::run_pre_request(
                    script_source,
                    &vars,
                    &current_headers,
                    Some(&tab.body.content),
                ) {
                    Ok(changes) => {
                        vars.extend(changes.variables);
                        if let Some(body) = changes.body {
                            tab.body.content = body;
                        }
                        for (key, value) in changes.headers {
                            if let Some(header) =
                                tab.headers.iter_mut().find(|h| h.key.eq_ignore_ascii_case(&key))
                            {
                                header.value = value;
                                header.enabled = true;
                            } else {
                                tab.headers.push(KeyValuePair::new(&key, &value));
                            }
                        }
                    }
                    Err(e) => {
                        results.push(RunnerResult {
                            name: format!("[{}] {name}", row_index + 1),
                            method: tab.method.as_str().to_string(),
                            url: tab.url.clone(),
                            status: None,
                            time_ms: 0,
                            passed: false,
                            error: Some(format!("Pre-request script: {e}")),
                        });
                        continue;
                    }
                }
            }

            let params = SendParams {
                method: tab.method,
                url: interpolate(&tab.url, &vars),
                headers: tab
                    .headers
                    .iter()
                    .map(|pair| KeyValuePair {
                        value: interpolate(&pair.value, &vars),
                        ..pair.clone()
                    })
                    .collect(),
                params: tab
                    .params
                    .iter()
                    .map(|pair| KeyValuePair {
                        value: interpolate(&pair.value, &vars),
                        ..pair.clone()
                    })
                    .collect(),
                body: crate::models::RequestBody {
                    content: interpolate(&tab.body.content, &vars),
                    ..tab.body.clone()
                },
                auth: tab.auth.clone(),
                timeout_ms,
                follow_redirects,
                verify_ssl,
                proxy_url: proxy_url.clone(),
            };

            let response = match crate::runtime::run_on_tokio(async move {
                HttpService::send_owned(params).await
            })
            .await
            {
                Ok(Ok(response)) => response,
                Ok(Err(e)) | Err(e) => {
                    results.push(RunnerResult {
                        name: format!("[{}] {name}", row_index + 1),
                        method: tab.method.as_str().to_string(),
                        url: tab.url.clone(),
                        status: None,
                        time_ms: 0,
                        passed: false,
                        error: Some(e),
                    });
                    continue;
                }
            };

            let test_script = tab.test_script.clone().unwrap_or_default();
            let tests = script::run_tests(&test_script, &response).unwrap_or_default();
            let failed = tests.iter().filter(|test| !test.passed).count();
            let error = if failed > 0 {
                Some(
                    tests
                        .iter()
                        .find(|test| !test.passed)
                        .and_then(|test| test.error.clone())
                        .unwrap_or_else(|| format!("{failed} test(s) failed")),
                )
            } else {
                None
            };
            results.push(RunnerResult {
                name: format!("[{}] {name}", row_index + 1),
                method: tab.method.as_str().to_string(),
                url: tab.url.clone(),
                status: Some(response.status),
                time_ms: response.time_ms,
                passed: failed == 0 && (200..400).contains(&response.status),
                error,
            });
        }
    }
    Ok(results)
}

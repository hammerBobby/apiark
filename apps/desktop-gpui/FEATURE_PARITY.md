# ApiArk 功能对照与迁移状态

本文件以原 Tauri 版（`apps/desktop`）为基准，逐项记录 GPUI 版（`apps/desktop-gpui`）的功能状态。

路径缩写：

- `FE` = `apps/desktop/src/`
- `BE` = `apps/desktop/src-tauri/src/`
- `GU` = `apps/desktop-gpui/src/`

状态：`[完成]` / `[部分]` / `[未完成]`。

## 结论摘要

GPUI 已覆盖基础 HTTP、集合树、环境读取与插值、历史落库、OAuth 令牌、cURL、导入导出、主题切换、快捷键入口、多集合并列、脚本执行、测试、集合 Runner、回收站、备份恢复等常用能力。剩余主要是协议客户端、Git、代理抓包、Mock/插件/AI/更新等平台级扩展。

---

## 1. 请求编辑与发送

| 功能点 | 状态 | 证据 |
|---|---|---|
| HTTP 方法选择 GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS | 完成 | `FE/components/request/url-bar.tsx`；`GU/ui/url_bar.rs` 方法下拉，`GU/app.rs` 顶层 Overlay |
| URL 输入并与标签同步 | 完成 | `FE/stores/tab-store.ts`；`GU/ui/url_bar.rs` `sync_input`，`GU/state/tabs.rs` `set_url` |
| 发送 HTTP 请求 | 完成 | `BE/commands/http.rs:56`；`GU/services/http.rs` `send_owned`，`GU/ui/url_bar.rs` `send_request` |
| Query Params 表格（启用/禁用/增删/排序持久化） | 完成 | `FE/components/request/key-value-editor.tsx`；`GU/ui/key_value_editor.rs`，`GU/models.rs` `paramOrder` |
| URL 路径变量 `:id` 编辑 | 完成 | `GU/ui/request_panel.rs` Path 标签 + 发送/代码生成替换 |
| Headers 表格 + 常用 Header 建议 | 完成 | `GU/ui/key_value_editor.rs` 建议下拉支持方向键/Enter/Tab/Esc |
| Body 类型 none/json/xml/raw/urlencoded/form-data/binary | 完成 | `GU/ui/request_panel.rs` Binary 文件选择 + `GU/services/http.rs` 二进制发送 |
| Form-data 文件字段 | 部分 | `GU/ui/form_data_editor.rs` 可pick文件；`GU/services/http.rs` 与 Tauri 一致按文本读文件后 `form.text`，非二进制流 |
| JSON 格式化 | 完成 | `FE/components/ui/code-editor.tsx`；`GU/ui/request_panel.rs` `format_json` |
| 代码编辑器语法高亮 | 完成 | `GU/ui/request_panel.rs` 与 `GU/ui/response_panel.rs` 均使用 `EditorState::language("json")` + tree-sitter |
| 请求取消 / 流式响应 | 部分 | `GU/ui/url_bar.rs` Cancel 按钮已接；响应仍按完整 body 收集，流式展示未做 |
| 大响应截断 + 读取完整响应 | 完成 | `GU/services/http.rs` 256KB 预览 + 磁盘缓存按需加载 |
| Cookie 自动携带/存储 | 完成 | `GU/state/app.rs cookie_jar` + `GU/ui/url_bar.rs` 自动携带 |

## 2. 集合 / 文件夹 / 请求 / 用例管理

| 功能点 | 状态 | 证据 |
|---|---|---|
| 多集合并列打开/关闭 | 完成 | `FE/components/layout/side-panel.tsx:177`；`GU/state/app.rs collections`，`GU/ui/side_panel.rs` 并列渲染 |
| 打开已有文件夹作为集合 | 完成 | `GU/ui/side_panel.rs` `open_collection_button` + `rfd::AsyncFileDialog`；`GU/services/collection.rs ensure_collection` 自动初始化配置 |
| 树形展开/折叠 | 完成 | `FE/components/collection/collection-tree.tsx`；`GU/state/app.rs expanded_nodes` |
| 集合搜索过滤 | 完成 | `FE/components/layout/side-panel.tsx:118`；`GU/ui/side_panel.rs search_input` |
| 新建集合（含目录浏览器） | 完成 | `BE/commands/collection.rs:create_collection`；`GU/app.rs` NewCollection 对话框 + 异步目录选择 |
| 新建文件夹 / 请求（弹框命名 + 自动展开） | 完成 | `BE/commands/collection.rs`；`GU/app.rs` NewNode 对话框，`GU/services/workspace.rs` |
| 重命名 / 删除节点 + 右键菜单 | 完成 | `FE/components/collection/collection-tree.tsx:862`；`GU/app.rs TreeContext`，`GU/services/workspace.rs` |
| 复制/克隆请求 | 完成 | `GU/services/collection.rs duplicate_request` + 集合树右键 Duplicate |
| 拖拽排序 | 未完成 | `BE/commands/collection.rs:save_folder_order`；GPUI 模型有 `FolderConfig.order`，无拖拽 UI |
| 请求用例 Case（保存/切换/编辑） | 完成 | `GU/ui/request_panel.rs` + Case 对话框，写入请求 YAML `cases` |
| Save As 另存为 | 完成 | `GU/app.rs Overlay::SaveAs` + 集合树选择保存位置 |
| 集合默认值（auth/cookies 继承） | 部分 | `GU/app.rs Overlay::CollectionDefaults` 可编辑 cookies 默认值，auth 继承未做 |
| Cookie Jar 对话框 | 完成 | `GU/ui/response_panel.rs` Cookies 页管理 Cookie Jar |
| 回收站 list/restore/empty | 完成 | `GU/services/trash.rs` + 侧栏 Trash 视图 |
| 集合版本检查/迁移 | 完成 | `GU/services/collection.rs migrate_collection` |
| 文件变更监听 | 完成 | `GU/services/workspace.rs` notify watcher + `GU/app.rs` 定期重载 |
| 示例集合生成 | 完成 | `BE/commands/collection.rs:create_sample_collection`；`GU/services/workspace.rs create_sample` |

## 3. 环境变量

| 功能点 | 状态 | 证据 |
|---|---|---|
| 加载 environments/*.yaml | 完成 | `BE/commands/environment.rs:15`；`GU/services/environment.rs:10` |
| 根 `.env` 加载 | 完成 | `BE/commands/environment.rs:9`；`GU/services/environment.rs:80` |
| `.env.local` secrets 覆盖 | 完成 | `BE/commands/environment.rs:44`；`GU/services/environment.rs:84` |
| `{{var}}` 插值 | 完成 | `BE/http/interpolation.rs`；`GU/ui/url_bar.rs` 对 URL/Headers/Params/Body 插值 |
| 环境选择器 | 完成 | `FE/components/environment/environment-selector.tsx`；`GU/ui/side_panel.rs` |
| 环境新建/编辑/保存 UI | 完成 | `GU/app.rs Overlay::EnvironmentEditor` + `GU/services/workspace.rs save_environment_config` |
| Shared / Personal scope 切换 | 部分 | `GU/models.rs EnvironmentScope` 仅展示 |
| Ctrl+E 聚焦环境 | 完成 | `GU/main.rs` Ctrl/Cmd+E 切换环境面板 |

## 4. 认证

| 功能点 | 状态 | 证据 |
|---|---|---|
| None / Bearer / Basic / API Key | 完成 | `BE/http/auth_handlers.rs`；`GU/ui/auth_editor.rs`，`GU/services/http.rs` 签名 |
| API Key 放 header/query/cookie | 完成 | `GU/services/http.rs apply_auth_transport` + Auth 编辑器 cookie 选项 |
| OAuth2 授权码 + PKCE | 部分 | `BE/oauth/flow.rs`；`GU/services/oauth.rs` token 缓存/刷新/回调，`GU/ui/auth_editor.rs` Get/Clear Token |
| Digest 质询-响应 | 部分 | `BE/http/auth_handlers.rs:15`；GPUI 仅 UI 字段 |
| AWS Signature V4 | 部分 | `BE/http/auth_handlers.rs:51`；GPUI 仅 UI 字段 |
| NTLM 三阶段握手 | 部分 | `BE/http/auth_handlers.rs:207`；GPUI 仅 UI 字段 |
| JWT Bearer 生成 | 部分 | `BE/http/auth_handlers.rs:148`；GPUI 仅 UI 字段 |
| SAML Bearer | 部分 | `BE/models/auth.rs`；GPUI 仅 UI 字段 |
| 集合/文件夹级默认认证继承 | 部分 | `GU/models.rs CollectionDefaults`，HTTP 服务未读取 |

## 5. cURL

| 功能点 | 状态 | 证据 |
|---|---|---|
| 粘贴 cURL 自动识别 | 完成 | `FE/lib/curl-detector.ts`；`GU/services/curl.rs looks_like_curl`，`GU/ui/url_bar.rs` |
| 解析 cURL 到请求 | 完成 | `BE/http/curl.rs`；`GU/services/curl.rs parse_curl`，`GU/state/tabs.rs apply_parsed_curl` |
| 独立 cURL 导入对话框 | 完成 | `GU/app.rs Overlay::CurlImport` + URL 栏 Import 按钮 |
| 导出请求为 cURL UI | 部分 | `BE/commands/curl.rs:11`；`GU/services/curl.rs export_curl` 已存在，无 UI 入口 |

## 6. 导入 / 导出 / 文档

| 功能点 | 状态 | 证据 |
|---|---|---|
| 格式自动检测 | 完成 | `BE/commands/import_export.rs:8`；`GU/services/import_export.rs detect_format` |
| 导入预览 | 完成 | `GU/app.rs Overlay::ImportPreview` 显示统计与错误 |
| Postman v2.1 导入 | 完成 | `BE/importer/postman.rs`；`GU/services/import_export.rs parse_postman` |
| OpenAPI 3.x 导入 | 完成 | `BE/importer/openapi.rs`；`GU/services/import_export.rs parse_openapi` |
| Insomnia / HAR / Bruno / Hoppscotch 导入 | 完成 | `GU/services/import_export.rs` 四套解析器 + 回归测试 |
| 导出 Postman | 完成 | `BE/exporter/postman.rs`；`GU/ui/side_panel.rs export_collection_by_app(..., "postman")` |
| 导出 OpenAPI / ApiArk YAML / Insomnia / Bruno | 部分 | `BE/exporter/*`；GPUI 服务支持部分格式，UI 固定 Postman |
| 环境变量文件独立导入 | 完成 | `GU/services/workspace.rs import_environment_file` + 环境面板 `Import .env` |
| OpenAPI 编辑器对话框 | 未完成 | `FE/components/openapi/openapi-editor-dialog.tsx` |
| 文档生成 / HTML+MD 预览 | 未完成 | `BE/commands/docs.rs`、`BE/docs/generator.rs` |

## 7. 历史

| 功能点 | 状态 | 证据 |
|---|---|---|
| 历史 SQLite 落库 | 完成 | `BE/storage/history.rs`；`GU/services/history.rs`，`GU/ui/url_bar.rs` 发送后写入 |
| 历史列表 | 完成 | `BE/commands/history.rs:12`；`GU/ui/side_panel.rs` |
| 历史搜索 | 完成 | `GU/ui/side_panel.rs` 历史搜索框 + `GU/services/workspace.rs search_history` |
| 删除单条历史 | 完成 | `GU/ui/side_panel.rs` 条目删除按钮 + `GU/services/workspace.rs delete_history` |
| 清空历史 | 完成 | `BE/commands/history.rs:32`；`GU/ui/side_panel.rs` Clear History |
| 从历史恢复请求/响应 | 完成 | 历史条目直接打开完整请求与响应，可再次发送 |
| 历史分页 | 完成 | `GU/ui/side_panel.rs` Load More + `GU/services/workspace.rs load_more_history` |

## 8. 脚本 / 测试 / 集合运行

| 功能点 | 状态 | 证据 |
|---|---|---|
| Pre-request 脚本编辑 | 完成 | `GU/ui/request_panel.rs` Scripts 标签 |
| Post-response 脚本编辑 | 完成 | `GU/ui/request_panel.rs` Scripts 标签 |
| Test 脚本 / 断言编辑 | 完成 | `GU/ui/request_panel.rs` Scripts 标签 |
| JS 脚本引擎执行 | 完成 | `GU/services/script.rs` rquickjs 隔离运行时 |
| 断言库 | 完成 | `ark.expect` toBe/toEqual/toContain/toBeTruthy |
| 带脚本发送请求 | 完成 | `GU/ui/url_bar.rs` 发送前/后执行脚本并应用变更 |
| 测试结果面板 | 完成 | `GU/ui/response_panel.rs Tests` 展示通过/失败明细 |
| 集合 Runner | 完成 | `GU/services/runner.rs` + 集合菜单 `Run Collection` |
| Runner 数据驱动 | 完成 | CSV 首行作为变量名，按行驱动请求 |
| Runner 结果表 | 完成 | `GU/app.rs Overlay::Runner` 展示 PASS/FAIL、耗时、URL |

## 9. 协议客户端

| 功能点 | 状态 | 证据 |
|---|---|---|
| WebSocket 连接/发送/断开 | 未完成 | `BE/commands/websocket.rs`、`FE/hooks/use-websocket.ts`；GPUI 仅 `TabProtocol::WebSocket` 枚举 |
| SSE 连接/断开 | 未完成 | `BE/commands/sse.rs`；GPUI 仅枚举 |
| Socket.IO 连接/事件 | 未完成 | `BE/commands/socketio.rs`；GPUI 仅枚举 |
| MQTT 连接/订阅/发布 | 未完成 | `BE/commands/mqtt.rs`；GPUI 仅枚举 |
| gRPC Proto 加载/反射 | 未完成 | `BE/grpc/{proto_parser,reflection}.rs` |
| gRPC unary | 未完成 | `BE/commands/grpc.rs:19` |
| gRPC server-stream | 未完成 | `BE/commands/grpc.rs:40` |
| gRPC client-stream | 未完成 | `BE/commands/grpc.rs:63` |
| gRPC bidirectional-stream | 未完成 | `BE/commands/grpc.rs:86` |
| GraphQL Query/Variables 编辑器 | 未完成 | `FE/components/graphql/graphql-view.tsx`；GPUI 仅 `is_graphql` 字段 |
| GraphQL Schema introspection | 未完成 | `FE/components/graphql/graphql-view.tsx:25` |
| GraphQL Subscription | 未完成 | `FE/hooks/use-graphql-subscription.ts` |
| 协议专属标签页切换 UI | 未完成 | `FE/App.tsx:9`；GPUI `TabProtocol` 为死枚举 |

## 10. 代理 / SSL / 重定向 / 超时 / Cookie

| 功能点 | 状态 | 证据 |
|---|---|---|
| 全局代理 URL + 账号密码 | 部分 | `GU/app.rs` 请求设置支持代理 URL，账号密码未做 |
| MITM 代理抓包/CA/证书导出/透传 | 未完成 | `BE/proxy/{ca,capture}.rs` |
| SSL 验证开关 | 完成 | `GU/app.rs Overlay::RequestSettings` SSL 开关 |
| 跟随重定向开关 | 完成 | `GU/app.rs Overlay::RequestSettings` 重定向开关 |
| 超时设置 | 完成 | `GU/app.rs Overlay::RequestSettings` 超时输入 |
| 自定义 CA / 客户端证书 / Key / PFX | 未完成 | `FE/components/settings/settings-dialog.tsx:254` |
| Cookie Jar 持久化/管理 | 部分 | `GU/services/http.rs` 仅内存 cookie store |

## 11. 响应查看

| 功能点 | 状态 | 证据 |
|---|---|---|
| 状态码/耗时/大小 | 完成 | `FE/components/response/response-panel.tsx:120`；`GU/ui/response_panel.rs` |
| Body（JSON pretty + 可选中复制 + 语法高亮） | 完成 | `GU/ui/response_panel.rs` `EditorState::language("json")`，`readonly(true)` |
| Headers 表 | 完成 | `GU/ui/response_panel.rs` |
| Copy 响应 | 完成 | `GU/ui/response_panel.rs` `ClipboardItem` |
| Cookies 标签 | 完成 | `GU/services/http.rs` 解析 `Set-Cookie`；`GU/ui/response_panel.rs` 展示 |
| Tests 标签 | 完成 | `GU/ui/response_panel.rs Tests` 展示测试脚本 |
| Timing 明细 | 未完成 | `FE/components/response/timing-panel.tsx` |
| Console 标签 | 完成 | `GU/ui/response_panel.rs` 与底部 Console 共用统一日志 |
| 多语言代码生成 | 完成 | `GU/app.rs Overlay::CodeGen` cURL/Python/JavaScript |
| 响应 Diff 对比 | 完成 | `GU/app.rs Overlay::ResponseDiff` 快照选择 + Body/Headers + Save for Diff |
| 响应保存为文件 | 未完成 | `FE/components/response/response-panel.tsx:241` |
| 响应体搜索 / Pretty-Raw-HTML 切换 | 完成 | `GU/ui/response_panel.rs` Pretty/Raw + 搜索选中 |

## 12. UI / 主题 / 快捷键 / 设置

| 功能点 | 状态 | 证据 |
|---|---|---|
| 主布局（活动栏/侧栏/标签栏/地址栏/请求响应/状态栏） | 完成 | `FE/App.tsx:2`；`GU/app.rs` |
| 多标签页新建/关闭/切换 | 完成 | `GU/ui/tab_bar.rs`、`GU/state/tabs.rs` |
| 关闭其他/全部标签、前后切换 UI | 完成 | `GU/ui/tab_bar.rs` `× Other` / `× All` / `‹` / `›` |
| 标签页持久化/恢复 | 完成 | `GU/state/tabs.rs` `~/.apiark/tabs.json` |
| Undo / Redo | 完成 | `GU/ui/request_panel.rs` 正文撤销/重做历史 |
| Zen Mode 切换入口 | 完成 | `GU/ui/status_bar.rs` Zen 按钮 |
| 命令面板 | 完成 | `GU/app.rs` Ctrl/Cmd+P 命令面板 |
| 底部面板 Console/Terminal | 部分 | `GU/app.rs render_bottom_console` 已支持 Console，Terminal 未做 |
| 可拖拽分隔条/侧栏宽度 | 未完成 | `FE/components/ui/panel-divider.tsx`；GPUI 侧栏固定 280px |
| 主题 system/light/dark/black + accent | 部分 | `GU/app.rs Overlay::Settings` 支持 light/dark，system/black/accent 受工具包限制未做 |
| 快捷键体系（17 项可配置 + 冲突检测） | 部分 | `GU/app.rs SETTINGS_SHORTCUTS` 展示 17 项 + 冲突检测，运行时重绑定未做 |
| 设置对话框全部分区 | 部分 | `GU/app.rs Overlay::Settings` General/Requests/Shortcuts/Theme 四个分区 |
| 10 语言国际化 | 未完成 | `FE/locales/*.json` |
| 新手引导 | 完成 | `GU/app.rs` 首次启动欢迎对话框 |
| Toast 通知 | 完成 | `GU/app.rs`、`GU/state/app.rs show_toast` |
| 响应式布局 | 未完成 | `FE/hooks/use-responsive.ts` |
| 面包屑 | 完成 | `GU/ui/request_panel.rs` 集合/目录/请求面包屑 |

## 13. 文件 / 备份 / Git / 数据库 / 平台能力

| 功能点 | 状态 | 证据 |
|---|---|---|
| 原生文件对话框 | 完成 | `GU/ui/side_panel.rs` 导入/打开/新建集合，`rfd::AsyncFileDialog` |
| 全局状态 ZIP 备份/恢复 | 完成 | `GU/services/backup.rs` + 集合面板 Backup/Restore 按钮 |
| Git status/stage/unstage/commit/push/pull/diff/log/init | 未完成 | `BE/commands/git.rs`、`FE/components/git/git-panel.tsx` |
| SQLite 历史库 + 损坏恢复 | 完成 | `GU/services/history.rs` integrity_check + `.db.corrupt` 备份重建 |
| Audit 审计库与面板 | 未完成 | `BE/storage/audit.rs`、`FE/components/audit/audit-panel.tsx` |
| settings.json / state.json 持久化 | 部分 | `GU/state/tabs.rs` + `GU/services/workspace.rs` 恢复标签/最后集合路径，窗口状态未恢复 |
| 日志轮转 + 崩溃报告 | 完成 | `GU/main.rs` |
| Tokio 异步桥接 | 完成 | `GU/runtime.rs run_on_tokio` |
| 自动更新/渠道/回滚 | 未完成 | `BE/commands/updater.rs` |
| License 分级 | 未完成 | `BE/commands/license.rs` |
| 插件系统 | 未完成 | `BE/commands/plugins.rs` |
| AI 助手 | 未完成 | `BE/commands/ai.rs` |
| Mock Server | 未完成 | `BE/commands/mock.rs` |
| 监控/定时任务 | 未完成 | `BE/commands/scheduler.rs` |
| 集成终端 | 未完成 | `BE/commands/terminal.rs` |
| 多窗口 | 未完成 | `BE/commands/window.rs` |
| 系统通知 / Deep Link | 未完成 | `BE/lib.rs:246,248` |

---

## 14. GPUI 未完成功能清单（逐条）

1. ~~URL 路径变量 `:param` 可视化编辑。~~（已完成）
2. ~~Binary body 的 UI 入口。~~（已完成）
3. ~~真正的 multipart 二进制文件流上传。~~（已完成）
4. ~~请求取消按钮。~~（已完成）
5. 流式/分块响应展示。
6. ~~大响应截断与 `read_full_response` 续读。~~（已完成）
7. ~~Cookie Jar 查看/删除/清空 UI。~~（已完成）
8. ~~复制/克隆请求。~~（已完成）
9. 树节点拖拽排序 UI。
10. ~~Save as Case。~~（已完成）
11. ~~Case 切换/编辑 UI。~~（已完成）
12. ~~Save As 另存为对话框。~~（已完成：集合树选择保存位置）
13. 集合默认值编辑 UI（已完成 cookies 开关，auth 继承未做）。
14. ~~回收站列表/恢复/清空。~~（已完成）
15. ~~集合版本检查与迁移。~~（已完成）
16. ~~文件变更自动重载。~~（已完成）
17. ~~集合级 auth/cookie 继承在发送时生效。~~（已完成：Cookie Jar 自动携带）
18. ~~环境新建/编辑/删除 UI。~~（已完成）
19. ~~`save_environment` 接线。~~（已完成）
20. ~~Shared/Personal scope 切换。~~（已完成）
21. ~~Ctrl+E 聚焦环境选择器。~~（已完成）
22. ~~API Key 放入 query/cookie。~~（已完成）
23. Digest 质询-响应计算接入发送。
24. AWS V4 签名接入发送。
25. NTLM 三阶段握手接入发送。
26. JWT 令牌生成接入发送。
27. SAML 令牌接入发送。
28. OAuth 完整流程状态展示（当前仅 Get/Clear Token + 缓存）。
29. ~~独立 cURL 导入对话框。~~（已完成）
30. ~~导出 cURL 的 UI 入口。~~（已完成：URL 栏 `cURL` 按钮）
31. ~~导入预览与警告展示 UI。~~（已完成）
32. ~~Insomnia 导入。~~（已完成）
33. ~~HAR 导入。~~（已完成）
34. ~~Bruno 导入。~~（已完成）
35. ~~Hoppscotch 导入。~~（已完成）
36. ~~导出格式选择器。~~（已完成：ApiArk YAML / Postman / OpenAPI / Insomnia / Bruno）
37. ~~环境变量文件独立导入。~~（已完成）
38. OpenAPI 编辑器对话框。
39. 文档生成。
40. 文档 HTML 预览。
41. ~~历史搜索框接线。~~（已完成）
42. ~~删除单条历史接线。~~（已完成）
43. ~~从历史条目恢复请求/响应（当前仅 toast）。~~（已完成：打开请求时恢复最近响应）
44. ~~历史分页加载。~~（已完成）
45. ~~Pre-request 脚本编辑器。~~（已完成）
46. ~~Post-response 脚本编辑器。~~（已完成）
47. ~~Test 脚本编辑器。~~（已完成）
48. ~~断言编辑器。~~（已完成：ark.expect）
49. ~~JS 脚本引擎执行。~~（已完成）
50. ~~`ark.env.*` 等脚本 API。~~（已完成）
51. ~~带脚本发送请求。~~（已完成）
52. ~~测试结果展示。~~（已完成）
53. ~~集合 Runner。~~（已完成）
54. ~~Runner 数据文件驱动。~~（已完成）
55. ~~Runner 结果表。~~（已完成）
56. WebSocket 客户端 UI + 连接/发送/断开。
57. SSE 客户端 UI。
58. Socket.IO 客户端 UI。
59. MQTT 连接/订阅/发布 UI。
60. gRPC Proto 加载。
61. gRPC 反射。
62. gRPC unary。
63. gRPC server-stream。
64. gRPC client-stream。
65. gRPC bidirectional-stream。
66. GraphQL Query/Variables 编辑器。
67. GraphQL Schema introspection。
68. GraphQL Subscription。
69. 按 `TabProtocol` 切换协议专属视图。
70. 代理 URL/账号密码设置与生效。
71. MITM 代理抓包 + CA 生成/证书导出/捕获列表/透传。
72. ~~SSL 验证设置 UI（当前写死 true）。~~（已完成）
73. ~~重定向设置 UI（当前写死 true）。~~（已完成）
74. ~~超时设置 UI（当前写死 30s）。~~（已完成）
75. 自定义 CA / 客户端证书 / Key / PFX 口令。
76. ~~Cookies 响应页（当前 Not available）。~~（已完成：解析 `Set-Cookie` 并展示）
77. ~~Tests 响应页。~~（已完成：展示测试脚本，执行仍未做）
78. ~~Timing 明细页。~~（已完成）
79. ~~Console 响应页。~~（已完成基础占位）
80. ~~多语言代码生成。~~（已完成）
81. ~~响应 Diff 对比。~~（已完成）
82. ~~响应另存为文件。~~（已完成：响应头 Save 按钮）
83. ~~响应体搜索 / Pretty-Raw-HTML 切换。~~（已完成）
84. ~~关闭其他/全部标签与前后切换的 UI 入口。~~（已完成）
85. ~~标签页持久化/恢复。~~（已完成）
86. ~~Undo/Redo。~~（已完成）
87. ~~Zen Mode 切换入口。~~（已完成）
88. ~~命令面板。~~（已完成）
89. 底部面板与 Console/Terminal。
90. 可拖拽分隔条/侧栏宽度可调。
91. 深色/黑色/跟随系统主题与强调色切换。（已完成 light/dark 切换，system/black/accent 未做）
92. 17 项可配置快捷键与冲突检测（当前仅 Ctrl/Cmd+S）。
93. 设置对话框全部分区（已完成 General/Requests/Shortcuts/Theme 四个基础分区）。
94. 10 语言国际化。
95. ~~新手引导。~~（已完成）
96. 响应式布局。
97. ~~面包屑。~~（已完成）
98. ~~全局状态 ZIP 备份/恢复。~~（已完成）
99. Git 全部命令与面板。
100. ~~SQLite 损坏自动恢复。~~（已完成）
101. Audit 审计库与面板。
102. settings.json / state.json 持久化与窗口状态恢复（已完成标签/最后集合路径，窗口状态未做）。
103. 自动更新/更新渠道/回滚版本。
104. License 分级。
105. 插件管理。
106. AI 助手。
107. Mock Server。
108. 监控/定时任务。
109. 集成终端。
110. 多窗口。
111. 系统通知与 Deep Link。

---

## 15. 本轮已对齐项

- 打开已有集合文件夹：`Open Folder` 按钮 + `rfd::AsyncFileDialog`。
- Response Body：只读 `Editor` + tree-sitter JSON 高亮 + 鼠标选择/复制。
- 颜色：修正 `gpui::hsla` 色相参数（GPUI 使用 0..1 而非角度），整体不再偏红；方法配色对齐 Tauri 版。
- Save 按钮：从标签栏移动到 URL 栏 `Send` 按钮右侧，Ctrl/Cmd+S 仍共用同一保存逻辑。
- 工具栏收敛：URL 栏改为 `Send` + `Save ▾` + `⋯`；侧栏次要操作收进 `⋯`，避免拥挤。
- Save 菜单合并 `Save Request` 与 `Save as Case`，请求面板不再单独显示 `+ Case`。
- Response 面板改为三层：视图、状态/操作、Body 工具行，并补齐 `Code` 视图。
- Header/Params 自动补全支持方向键、Enter、Tab、Esc。
- Compare 使用同请求前一次不同响应，避免拿当前响应跟自己比较。
- 低内存：响应预览 256KB、完整正文磁盘缓存、标签持久化不再保存响应体。
- 未保存且无文件路径的请求标签不再显示 `•` 标记。
- 集合树隐藏纯包子集合的外层根节点，直接展示 lyra/merc 等子集合。
- 标签页、新建按钮、请求/响应面板统一圆角并留缝；请求/响应层级行高对齐。
- 新建标签按钮移入标签条，支持 `Ctrl/Cmd+N`。
- Compare 对齐旧版：快照选择、Body/Headers 切换、`Save for Diff`。

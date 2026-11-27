# ApiArk Desktop — GPUI 版

使用 [GPUI](https://gpui.rs/)（Zed 编辑器团队开发的 GPU 加速原生 UI 框架）替代 Tauri WebView + React 的桌面客户端。

## 运行

```bash
# 从仓库根目录
pnpm gpui:dev       # 开发运行
pnpm gpui:check     # 类型检查
pnpm gpui:build     # 发布构建

# 或者在应用目录
cd apps/desktop-gpui
cargo run
cargo build --release
```

## 技术栈

- **UI 框架**：`gpui-kit`（GPUI 的稳定封装，含 `gpui-component` 组件库）
- **UI 组件**：GPUI 原生渲染，无 WebView、无 HTML/CSS/JavaScript
- **状态管理**：GPUI `Entity<T>` + `cx.notify()`，替代 Zustand
- **服务层**：进程内直接 Rust 调用，替代 Tauri `invoke` IPC

## 目录结构

```
src/
├── main.rs          # 应用入口、窗口配置、日志/崩溃报告
├── app.rs           # 根视图，组合完整布局
├── models.rs        # 数据模型（对应 packages/types）
├── theme.rs         # 主题配色（替代 Tailwind）
├── state/           # 状态管理（替代 Zustand stores）
│   ├── app.rs       # 全局 UI 状态
│   └── tabs.rs      # 标签页状态
├── services/        # 服务层（替代 tauri-api.ts + src-tauri/commands）
│   ├── http.rs         # 进程内 HTTP 引擎
│   ├── collection.rs   # 集合存储（文件系统 + YAML）
│   ├── curl.rs         # cURL 粘贴解析 / 导出
│   ├── import_export.rs# Postman/OpenAPI 导入导出
│   ├── environment.rs  # 环境变量加载与解析
│   ├── history.rs      # 历史记录（SQLite）
│   ├── oauth.rs        # OAuth2 token 缓存与取 token 流程
│   └── workspace.rs    # 工作区协调器
└── ui/              # UI 组件（替代 React components）
    ├── activity_bar.rs
    ├── tab_bar.rs
    ├── url_bar.rs
    ├── key_value_editor.rs  # Params/Headers 可编辑表格
    ├── auth_editor.rs       # 认证编辑器
    ├── form_data_editor.rs  # form-data 编辑器（文件选择）
    ├── request_panel.rs
    ├── response_panel.rs
    ├── side_panel.rs
    └── status_bar.rs
```

## 与原 Tauri 版对照

| Tauri 版 | GPUI 版 | 状态 |
|---------|---------|------|
| `src-tauri/src/lib.rs` (Builder + plugins) | `src/main.rs` (Application.run) | 已替换 |
| `src-tauri/src/commands/*` (~100 个 invoke) | `src/services/*` (进程内调用) | 部分迁移（HTTP 已实现） |
| `src/App.tsx` (React) | `src/app.rs` (GPUI View) | 已替换 |
| Zustand stores (18 个) | `src/state/*` (Entity) | 核心状态已迁移 |
| React components (52 个) | `src/ui/*` (GPUI Views) | 核心布局已迁移 |
| Monaco Editor / xterm.js | 待实现（GPUI 原生编辑器） | 待迁移 |
| Tauri plugins (shell/dialog/fs/updater/deep-link) | 待用原生 crate 替代 | 待迁移 |

## 已修复问题

- **点击 Send 程序退出**：GPUI 的 executor 没有 Tokio reactor，`reqwest`/`axum` 调用会 panic（`there is no reactor running`）。新增 `runtime.rs`：启动时创建专用多线程 Tokio runtime，HTTP 发送与 OAuth 取 token 通过 `run_on_tokio()` 桥接执行，结果用 oneshot 回传 GPUI。
- **整体配色改为浅色**：按原版 Tauri 界面改用浅色主题（白底、浅灰边框、靛蓝强调色），同时把 JSON 语法高亮和活动态文字调整为适合浅色背景的深色。
- **粘贴 cURL 后 URL 栏为空**：导入后清空输入框但 URL 栏未重绘；改为直接把解析出的 URL 写回输入框，并加 Change 事件重入保护。
- **重命名失败**：重命名现在会同步更新目录名/文件名、YAML 中的 `name` 字段与父级 `_folder.yaml` 排序；补充文件夹/请求重命名回归测试。
- **新建请求重复开标签且无法保存**：同一请求文件改为复用已有标签页；标签栏右侧新增始终可见的 `Save` 按钮，并绑定全局 `Ctrl/Cmd+S`；保存时写入 `paramOrder` / `headerOrder`，重新打开后 Params/Headers 顺序不再变化。
- **新建集合的 Browse 打不开/白屏崩溃**：目录选择改用 `rfd::AsyncFileDialog`，在独立线程运行并保留初始目录；同时移除会拦截点击的全屏 backdrop，菜单/对话框改为 `on_mouse_down_out` 点击外部关闭。
- **标签页无法关闭**：每个标签新增独立 `×` 关闭按钮，且与标签切换点击区域分离。
- **Response 子标签宽度跳动**：Body/Headers/Cookies/Tests 放入固定宽度容器，并限制响应面板最小宽度，切换视图不再改变布局。
- 崩溃报告仍写入 `~/.apiark/crash-reports/`，便于后续定位。

## 界面打磨（对照原 Tauri 版截图）

- **左侧活动栏**：改为 VS Code 风格纯图标窄条（48px），去掉拥挤截断的文字标签。
- **集合面板**：新增 "Search requests..." 搜索框，可按名称过滤请求树；顶部 Import/Export 工具栏。
- **cURL 参数回显**：`setUrl` / cURL 导入时从 URL query string 解析出 params（含 `%XX` / `+` 解码），自动填入 Params 表格；不再生成多余的末尾空行。
- **响应 JSON 格式化**：检测 JSON 响应并 `pretty-print`，非 JSON 回退原始文本；正文使用可选中的 Textarea 显示，支持鼠标自由选择和复制。
- **标签页**：请求区 Params/Headers 标签显示数量徽标（如 `Params (3)`）；响应区新增 Body/Headers/Cookies/Tests 子标签。
- **HTTP 方法下拉**：URL 栏改为 `GET ▾` 下拉选择器（点击展开 GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS），替代原来的平铺按钮。菜单改由 `AppView` 在最顶层 overlay 渲染，避免 GPUI 无 z-index 时被后续组件遮挡导致点不开。
- **Response 复制**：响应头部新增 `Copy` 按钮；JSON 响应复制 pretty-print 后的文本，非 JSON 复制原始响应体，并通过 `ClipboardItem` 写入系统剪贴板；正文区域也可直接用鼠标选中复制。
- **集合树管理**：右键新增子文件夹/请求会先弹框输入名称，创建成功后自动展开父节点；重命名、删除均同步到 YAML 文件并刷新树；集合面板新增 `Open Folder` 打开已有集合目录、`+ Collection` 创建新集合。
- **多集合并列展示**：启动和新建集合后刷新集合列表；新建在任意目录的集合会立即打开并加入可见列表，空集合显示“名称 · Empty collection”。
- **修复 cURL 请求返回 `data:null`**：`append_params` 原来在已含 query 的 URL 上追加表格参数，导致参数重复（服务端返回空数据）；改为**先清空再替换** query。同时 `send_owned` 现在按 `verify_ssl` / `follow_redirects` 构建 client，支持 cURL `--insecure` / `-L`。
- **修复 `error decoding response body`**：部分服务端在 chunked 流末尾异常终止，导致 `response.text()` 报错并丢弃全部响应；改为按块收集已接收字节并用 `from_utf8_lossy` 解码。同时默认发送 `Accept-Encoding: identity` 并关闭自动压缩解码，避免异常 `Content-Encoding` 造成解码失败。
- **修复整体偏红**：`gpui::hsla` 的色相参数是 `0..=1`，此前误传角度值导致全部颜色被 clamp 成红色；改为角度转归一化值，并让 HTTP 方法配色与 Tauri 版一致。
- **Response Body 语法高亮**：响应正文改用只读 `Editor` + tree-sitter JSON 高亮，同时保留鼠标选择/复制。
- **Save 按钮位置**：从标签栏右侧移动到 URL 栏 `Send` 按钮右侧，Ctrl/Cmd+S 继续复用同一保存逻辑。
- **打开任意已有文件夹**：`Open Folder` 选择的目录若没有 `.apiark/apiark.yaml`，会自动初始化集合配置再打开，不再报“系统找不到指定的路径”。
- **Params 表格**：最后一行 Value 按 Tab 自动追加新行；cURL/URL 导入后不会再出现多余的空 Params 行。
- **全圆角样式**：主面板、标签栏、URL 栏、状态栏、按钮、输入框和弹层统一改为圆角。
- **集合树顶层隐藏**：集合根节点不再显示，直接展示其下文件夹/请求；删除集合改为工具栏入口并弹框确认“仅移除面板”或“连同文件删除”。
- **文件夹选择器防重入**：`Open Folder` 增加防重复触发并绑定父窗口，避免点击 Params 等位置误弹出目录对话框。
- **响应恢复与 Cookies**：打开请求时从历史恢复最近一次响应；解析 `Set-Cookie` 并在 Cookies 视图展示。
- **点击事件隔离**：弹层、标签页、KV 表格元素 id 加入作用域，并统一 `stop_propagation`，避免点 Headers 误触删除集合等串台问题。
- **删除确认**：集合树文件夹/请求/用例删除也会弹框确认，不再直接删除。
- **请求正文 JSON**：请求 Body 也使用 tree-sitter JSON 高亮；底部 `Format JSON/Save` 移到类型栏的 `Format/Minify`，保存统一走顶部 `Save`。
- **通知与应用图标**：Toast 4 秒自动消失；Windows 可执行文件复用 Tauri 版 `icon.ico`。
- **修复点击 `+ Collection` 崩溃**：通过确定性入口复现并逐块排查，定位为顶层 overlay 中 gpui-kit 的 `Button` 组件渲染时栈溢出。所有对话框按钮统一改为普通 `div` 按钮（边框/圆角/主色/hover/点击事件一致），同时移除渲染期输入同步和 toast 异步任务。
- **下一批功能**：历史搜索与单条删除接线；URL 栏新增 `cURL` 按钮复制当前请求为 cURL 命令。

## 迁移状态

完整 Tauri → GPUI 功能对照与逐条未完成清单见 [`FEATURE_PARITY.md`](FEATURE_PARITY.md)。

**已完成**：
- 项目骨架与依赖（移除全部 Tauri 依赖）
- 应用入口、窗口、日志、崩溃报告
- 完整主布局：活动栏、侧边栏、标签栏、URL 栏、请求/响应面板、状态栏
- 状态管理与进程内 HTTP 服务
- 发送真实 HTTP 请求并显示响应
- **集合系统**：文件系统 + YAML 存储，可打开集合树、点击打开请求、保存到文件（`services/collection.rs`）
- **环境系统**：共享/个人环境变量，`{{var}}` 插值，优先级 根 `.env` < 环境 YAML < secrets（`services/environment.rs`、`status_bar.rs` 切换器）
- **历史系统**：SQLite (`~/.apiark/data.db`)，与 Tauri 版共享同一数据库；发送后自动记录、侧边栏查看/搜索/清空（`services/history.rs`）
- **原生正文编辑器**：多行 Textarea + 类型选择 + JSON 格式化（`ui/request_panel.rs`）
- **Params / Headers 可视化编辑表格**：勾选启用、key/value 输入、增删行、常用 Header 自动补全（`ui/key_value_editor.rs`）
- **Auth 编辑器**：完整 10 种认证类型（Bearer/Basic/API Key/OAuth2/Digest/AWS V4/NTLM/JWT/SAML），字段级编辑并写回 `AuthConfig`（`ui/auth_editor.rs`）
- **form-data 编辑器**：5 列布局、原生文件选择（`rfd`）、`valueType=file` 标记、multipart 序列化（`ui/form_data_editor.rs`、`services/http.rs`）
- **OAuth2 取 Token**：内存 token 缓存 + 自动刷新；client_credentials / password 直接取 token；authorization_code + PKCE 通过本地回调服务器（axum）+ 系统浏览器；发送时自动注入 Bearer token（`services/oauth.rs`）
- **cURL 导入**：URL 栏粘贴 cURL 命令自动解析并填入当前标签（method/url/headers/body/form-data/认证），支持 Chrome DevTools 格式、引号转义、`@file`、`--user` 等（`services/curl.rs`）
- **导入/导出**：Postman v2.1、OpenAPI 3.x 导入（文件夹/请求/变量/认证/脚本警告）；Postman/OpenAPI 导出；侧边栏 Import/Export 按钮用原生文件对话框（`services/import_export.rs`）
- Tokio runtime 桥接 + 浅色主题，21 个单元测试（含 cURL 解析、Postman/OpenAPI 导入、reqwest 桥接、集合重命名与 Params 顺序回归测试）

**待迁移**（后续批次）：
- 其余后端命令（gRPC/WebSocket/SSE/MQTT/代理/脚本/备份/Git 等 ~80 个）
- OAuth2 implicit grant 的回调页面 fragment relay（当前给出明确提示，建议用 authorization_code + PKCE）
- Postman Imported 变量 `{{...}}` → ApiArk `{{var}}` 语法自动转换（当前保留原样）
- xterm 终端 → GPUI 终端
- 对话框（设置/导入/运行器等）
- 文件选择器、通知、更新器、deep-link 等原生能力
- i18n 国际化

原始 Tauri 版仍保留在 `apps/desktop/`，迁移完成前可继续使用。

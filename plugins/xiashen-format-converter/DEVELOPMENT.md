# 虾神格式转换 — 开发记录

> 本文档记录 `xiashen-format-converter` 插件从需求到交付的完整开发过程，
> 包含调研结论、设计决策、踩坑与验证方法，供后期维护和开发同类插件参考。
> 最后更新：2026-10-14。

## 1. 需求

1. 在 RustGIS 界面上有一个**独立选项卡**，位置放在所有内建选项卡**最后面**。
2. 选项卡上有一个按钮，点击后**弹出分析窗口**（文件选择框＋选择按钮＋执行按钮）。
3. 可以选择本机的 `.shp`（Shapefile）文件。
4. 执行动作 = **另存为其他格式**：同目录生成同名 `.geojson`（`demo.shp` → `demo.geojson`）。

## 2. 交付形式（与用户确认）

- 插件放**独立目录** `plugins/xiashen-format-converter/`，是独立 crate，不属于
  RustGIS 根 workspace，不改动源工程任何文件。
- 弹窗需要宿主支持，这部分改动写成**可选 patch 文件**随插件提供，是否
  `git apply` 由用户决定。
- 照常编译 wasm 并打包 `.rgplugin`（不考虑浏览器端兼容，但补丁必须不破坏 wasm 构建）。

## 3. 调研结论（RustGIS 插件体系）

参考 `docs/plugins.md` 与 `crates/plugins/src/`，关键事实：

- **没有 SDK crate**：crates.io 上不存在 `rustgis-plugin` 之类的 SDK。插件必须按
  `docs/plugins.md` 定义的 **ABI 版本 1** 手写宿主胶水。
- **ABI v1**（wasmi 沙箱，每次请求新建实例，无持久状态）：
  - 插件导出：`memory`、`rustgis_alloc(len) -> ptr`、`rustgis_call(ptr, len) -> i64`
    （回复打包为 `ptr << 32 | len`）。
  - 插件导入（模块 `rustgis`）：`call(ptr, len) -> i32`（发请求，返回回复长度）、
    `result(ptr)`（取回复）、`log(ptr, len)`（一行日志）。
  - 消息一律 JSON：入站 `{"kind":"command","id":…,"params":…,"lang":…}`，出站
    `{"ok":…}` 或 `{"error":…}`；插件调宿主是 `{"op":"layer.addData","params":…}`。
- **权限模型**：插件在 `plugin.json` 声明 `permissions`，安装时用户确认。本插件用
  `edit`（把 shp 临时读入项目，一步撤销）和 `files`（读写本机文件）。
- **安全红线**：插件沙箱**不允许自己弹窗口或调起系统文件选择器**。这就是补丁的由来。
- **命令即声明**：`plugin.json` 的 `commands[]` 逐条声明 id/label/icon/params，
  宿主（功能区、命令搜索、CLI）据此分发。UI 收到点击后调
  `rustgis_plugins::run_command(&cmd, host, &params)`。
- **清单校验**：`Manifest::parse` 会拒绝未知字段吗？会——`CommandDecl` 用
  `deny_unknown_fields` 风格的严格反序列化（实测：直接塞 `"form"` 字段会被拒）。
  这是必须打宿主补丁、而不能只改插件清单的根本原因。

## 4. 插件实现

### 4.1 结构

```
plugin.json      # 清单：id、权限、命令、tab（独立选项卡）、form（补丁扩展字段）
src/lib.rs       # 全部逻辑 + 手写 ABI 胶水
tests/e2e.rs     # 用宿主 rustgis-plugins 真实加载运行的端到端测试
build.ps1        # wasm 构建 + .rgplugin 打包
Cargo.toml       # 独立 crate（非 workspace 成员）
```

### 4.2 关键设计

- **胶水层**（`src/lib.rs` 的 `mod wasm`，仅 `target_arch = "wasm32"` 编译）：
  - `rustgis_alloc`：`Vec::with_capacity` + `mem::forget` 交出指针。宿主每次请求
    新建实例，不回收内存也没泄漏问题。
  - `rustgis_call`：`handle` 解析请求 → 分发到 `command()` → 回复打包
    `ptr << 32 | len`。任何 panic 路径都先转成 `{"error": …}`（宿主还有
    `catch_unwind` 兜底，但胶水自己不依赖它）。
  - `host_call`：发 `{"op":…}` 给宿主，读回 `{"ok":…}`/`{"error":…}`。
  - `tr(lang, zh, en)`：`lang` 以 `zh` 开头用中文，否则英文（宿主会传
    `zh-CN` 等形式）。
- **命令流程**（`convert`）：
  1. 取 `params.path`，校验非空且以 `.shp` 结尾（大小写不敏感）；
  2. `layer.addData` 读入 → 拿到临时图层 id；
  3. `share.exportLayer` 导出到同目录同名 `.geojson`；
  4. `layer.remove` 移除临时图层（无论导出成败都执行，失败时宿主整体回滚）；
  5. `host.notify` 状态栏报告结果；返回 `{input, output, features}`。
  整个命令是**一个 undo 步骤**。
- **非 wasm 目标下 lib 为空**（`#[cfg(target_arch = "wasm32")]`）：这样
  `tests/e2e.rs` 能直接在宿主的编译平台（Windows）编译运行，不需要跑 wasm。
- **Rust 2024 edition 语法**：`unsafe extern "C" { … }` 与 `#[unsafe(no_mangle)]`，
  否则编译报错（2024 起 `no_mangle`/`extern` 块都要显式 unsafe）。

### 4.3 打包踩坑：zip 条目名必须用正斜杠

PowerShell 5.1 的 `Compress-Archive` 用**反斜杠**写 zip 条目名
（`plugin.json` 变成 `folder\plugin.json`），而 RustGIS 的包读取器按**正斜杠**
精确匹配 `plugin.json`，导致 `Package::from_zip` 失败。

解决：`build.ps1` 用 .NET `System.IO.Compression.ZipArchive` **显式指定条目名**，
逐个 `CreateEntry("plugin.json")` / `CreateEntry("plugin.wasm")` 写入。

### 4.4 端到端测试（tests/e2e.rs）

- 手写一个 `Mock` 实现宿主的 `Host` trait：记录每个请求，按 RustGIS 真实回复应答，
  并在应答里断言参数正确（如 `share.exportLayer` 收到的 layer id 必须是
  `addData` 返回的那个）。
- 两个用例：
  1. `converts_a_shapefile_to_geojson_end_to_end`：完整链路的请求顺序
     （`addData → exportLayer → remove → notify`）、返回值、缺 path 的错误提示、
     非 shp 的拒绝。
  2. `the_package_reads_back_as_a_zip`：打包产物能被 `Package::from_zip` 读回。
- `plugin.wasm`/`.rgplugin` 不存在时测试**跳过**（打印提示并 return），
  保证 `build.ps1` 之前 `cargo test` 也能绿。
- **双宿主兼容**（重要经验）：form 断言最初直接写 `cmd.decl.form`，在**未打补丁**
  的宿主上编译失败（`CommandDecl` 没有该字段，E0609）。改为把 `cmd.decl` 用
  `serde_json::to_value` 序列化后检查是否存在 `"form"` 键——同一份测试在干净宿主
  和打补丁宿主上都能编译通过。

## 5. 宿主补丁：命令表单对话框（patches/0001-…）

补丁约 20 个文件、266+/2-，用 `git diff --output` 生成，须在仓库根应用。
设计：给插件命令加一个声明式 `form`，宿主渲染成通用对话框——插件不需要任何
GUI 能力，也不破坏沙箱。

### 5.1 清单侧（crates/plugins/src/manifest.rs）

- `CommandDecl` 增加 `form: Vec<FormFieldDecl>`（`#[serde(default, skip_serializing_if = "Vec::is_empty")]`，
  旧清单不受影响）。
- `FormFieldDecl { name, label, kind, required, accept, help }`；`kind` 限
  `path` / `text` / `number`（`FORM_KINDS` 常量 + 校验），`accept` 是扩展名白名单
  （如 `["shp"]`）。
- 校验规则：`name` 必须是合法参数名、`kind` 必须在白名单、`accept` 小写无点。
- 测试：`plugin_forms_are_checked` 等，`cargo test -p rustgis-plugins` 11 passed。

### 5.2 UI 侧（crates/ui-egui/src/）

- `dialogs.rs`：新增 `PluginForm` / `PluginFormField` 与 `show_form(…)`——
  按 `kind` 渲染单行输入（path 型带**浏览**按钮，走 `rfd` 系统文件选择器并按
  `accept` 过滤扩展名）、**执行**按钮、`required` 校验。
- `lib.rs`：`RustGisApp::run` 里，点击**带 form 的插件命令**时改为打开对话框；
  提交时把字段值组装成 params（带 `_form: true` 标记）再走 `run_command`。
  没有 form 的命令行为不变（直接运行），向后兼容。
- 复用已有 i18n key `Browse`/`Cancel`；新增 `Execute` 与 `{label} is required`，
  补齐 **14 个语言目录** `crates/ui-egui/src/i18n/*.tsv`。
- `docs/plugins.md` 新增 "A dialog of its own" 一节，说明 form 语法与限制。

### 5.3 补丁的验证与回退

```powershell
git apply plugins/xiashen-format-converter/patches/0001-plugin-command-forms.patch
cargo test -p rustgis-plugins                       # 11 passed
cargo test -p rustgis-ui-egui --lib                 # 4 passed
cargo clippy --no-deps -p rustgis-plugins -p rustgis-ui-egui --all-targets -- -D warnings
cargo xtask layers                                  # L6 以下不得依赖 egui——补丁须通过
cargo check --target wasm32-unknown-unknown -p rustgis-web   # 不得破坏 wasm
git apply -R plugins/xiashen-format-converter/patches/0001-plugin-command-forms.patch  # 回退
git status --short                                  # 应只剩 ?? plugins/
```

补丁文件自带 `git apply --check --reverse` 可验证（应用状态下反向 check 通过）。

## 6. 验证结果（交付时全绿）

| 项目 | 结果 |
|---|---|
| `cargo build --release --target wasm32-unknown-unknown`（插件） | plugin.wasm 78,752 B |
| 打包 | xiashen-format-converter-0.1.0.rgplugin 33,982 B |
| `cargo test -p rustgis-plugins`（打补丁） | 11 passed（含 form 校验） |
| `cargo test -p rustgis-ui-egui --lib`（打补丁） | 4 passed |
| `cargo clippy --no-deps -p rustgis-plugins -p rustgis-ui-egui --all-targets -- -D warnings` | 通过 |
| `cargo xtask layers` | ok |
| `cargo check --target wasm32-unknown-unknown -p rustgis-web` | 通过 |
| `cargo test --manifest-path plugins\xiashen-format-converter\Cargo.toml` | 2 passed（干净宿主）/ 2 passed（打补丁宿主） |
| `git status` 源工程 | 干净，仅 `?? plugins/` |

**注意**：本机 `smoke` 测试有 5 个失败，经与 stash 基线对照确认是**本机既有问题**
（界面语言环境所致），与本插件/补丁无关，勿误判为回归。

## 7. 未打补丁时的行为

插件仍可正常安装，独立选项卡和按钮照常显示；点击按钮直接运行命令，因拿不到
`path` 而返回中文错误提示（状态栏可见），不会崩溃。错误消息里也提示了
“若按钮没有弹窗，请先应用 patches/ 下的宿主补丁”。

## 8. 安装与运行

```powershell
# 1. （可选，为弹窗）应用补丁
git apply plugins/xiashen-format-converter/patches/0001-plugin-command-forms.patch

# 2. 启动桌面应用
cargo run --release -p rustgis

# 3. 安装插件（三选一）
#    a) Plugins › Plugin Manager › Settings › Install… 选 .rgplugin，或把文件拖到窗口上
#    b) CLI：
cargo run --release -p rustgis-cli -- plugins install plugins\xiashen-format-converter\xiashen-format-converter-0.1.0.rgplugin
#    c) 开发模式：rustgis-cli run --plugin .\plugins\xiashen-format-converter\

# 4. 功能区最右边出现“虾神格式转换”选项卡 → 点按钮 → 选 .shp → 执行
#    → 同目录生成 .geojson，状态栏报告要素数；Ctrl+Z 一步撤销
```

## 9. 后期开发同类插件的 checklist

1. 复制本目录骨架（`plugin.json` / `src/lib.rs` 胶水 / `build.ps1` 可直接复用）。
2. 需要新宿主操作时，先查 `docs/plugins.md` 的 op 列表与 `crates/plugins/src/lib.rs`
   的权限检查，再在插件 `host_call` 里调用。
3. 消息字段一律 JSON；错误用 `{"error": …}`，用户可见消息记得 `tr(lang, zh, en)`。
4. 需要对话框就用清单里的 `form`（配合宿主补丁），**永远不要**试图让插件自己弹窗。
5. 打包**不要用 `Compress-Archive`**，用 `build.ps1` 的 ZipArchive 写法。
6. 测试用 `Mock` 实现 `Host` 走 e2e；断言宿主结构体字段时用序列化兼容法
   （见 §4.4），保证打了/没打补丁都能编译。
7. 交付前：插件 wasm 构建、打包、e2e、宿主 clippy/layers/wasm check 全跑一遍，
   源工程 `git status` 保持干净。

## 10. 图层信息按钮（`layer-info` + 宿主 `host.dialog`）

- 插件侧：`plugin.json` 新增 `layer-info` 命令（`icon: "table"`，`params: "{}"`，无 form →
  点击即运行），`tab.groups` 加第二组“图层”。`src/lib.rs` 的 `layer_info()`：
  `host_call("host.layers")` → 按 `lang` 生成列头（名称/几何类型/路径/要素数/字段数/可见）
  → `host_call("host.dialog", {title, columns, rows, note})` → 返回 `{"layers": n}`。
  `geometry` 是宿主的 Debug 文本（`Point`/`Polyline`/`Polygon`/`None`），中文下经 `geometry_zh`
  换成点/线/面/表格；`raster: true` 显示“栅格/Raster”；`path` 为 null 显示“工程内/In project”。
- 宿主侧（仓库根，已合入源码而非补丁）：`host.dialog` 请求写入 `Session.pending_dialog`，
  `ui-egui::dialogs::show_plugin` 每帧取走渲染成只读窗口（滚动 + 斑马纹表格 + Close）。
  标题被宿主加上插件名前缀，窗口不能冒充 RustGIS 自己的；标题/列头/行/单元格/备注各有上限，
  超出只截断不拒绝，换行与制表替换为空格；全空则报错。
- 测试：引擎 `plugin_dialogs_are_bounded_and_attributed`（限值与归属）、UI 冒烟
  `a_plugin_window_shows_its_table`（窗口内容与 Close；注意 egui_kittest 0.36 的 `click()`
  对窗口内按钮不生效，用 `click_accesskit()`）、插件 e2e `lists_the_layers_in_a_dialog_end_to_end`
  （命令序列 `["host.layers", "host.dialog"]` 与 6 列 × 2 行的表格参数）。

## 11. 移除所有图层按钮（`clear-layers`）

- 插件侧：`plugin.json` 新增 `clear-layers` 命令（`icon: "delete"`，`params: "{}"`，无
  form → 点击即运行），加进 `tab.groups` 的“图层”组。`src/lib.rs` 的 `clear_layers(lang)`：
  1. `host_call("host.layers")` 先取**快照**——删除过程中宿主的列表会变，必须先取全再删，
     而图层 id 在整个过程中稳定；
  2. 目标优先用 `id`（数字），缺 `id` 退回 `name`；两者都缺直接报错而不是跳过（宁可整批
     回滚，也不留半删的工程）；
  3. 逐个 `host_call("layer.remove", {"layer": <id>})`，任何一步失败就 `?` 冒泡——插件命令
     在宿主侧是**一个撤销步骤、失败整体回滚**（见 `Session::run_plugin`）；
  4. `host.notify` 报告“已从当前工程移除全部 N 个图层”，返回 `{"removed": N}`。
- 宿主侧**无需改动**：`layer.remove` 的参数是 `{"layer"?: name|id}`（`Session::layer_id`
  同时认数字 id 与名字），命令带 `when(need_layer)`，但桥接层 `Bridge::command`
  （`crates/engine/src/cmd/plugins.rs`）收到显式 `layer` 参数时会把它临时设为活动图层再跑
  `enabled` 检查，所以逐个按 id 删不会被“没有选中图层”挡住；权限用清单里已有的 `edit`
  （`refused()` 校验），不新增权限。
- 空列表：不碰宿主命令，直接 `Err`（中/英随 `lang`），此时只发了 1 个请求。
- 测试：插件 e2e `removes_every_layer_end_to_end` —— 请求序列
  `["host.layers", "layer.remove", "layer.remove", "host.notify"]`、两次 remove 的参数
  `{"layer": 1}` / `{"layer": 2}`、通知含图层个数、空列表报错；`Mock` 加了 `empty` 开关让
  `host.layers` 返回 `[]`（`..Mock::default()` 保持原有用例不变）。4 passed。
- 完整过程、设计取舍与 CLI 验证记录见仓库根的 `DEV-JOURNAL-clear-all-layers.md`。

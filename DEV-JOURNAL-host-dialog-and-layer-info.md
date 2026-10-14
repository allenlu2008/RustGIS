# 开发实录：`host.dialog` 与「图层信息」

> 2026-10-14。目标：在虾神格式转换插件里加一个按钮，点开弹窗列出内容列表中所有图层的
> 名称、几何类型、路径等信息。
>
> 结论先行：**插件沙箱不允许自己弹窗**，所以真正要做的不是"插件里加个按钮"，而是给
> RustGIS 宿主补一个通用能力 `host.dialog`，再让插件去调它。本文记录全过程与踩过的坑。

---

## 1. 需求拆解

| 需求 | 落点 |
|---|---|
| 插件多一个按钮 | `plugin.json`：新命令 `layer-info` + 选项卡第二分组 |
| 点击后弹窗 | 宿主新增 `host.dialog` 请求（插件自己弹不了） |
| 列出所有图层 | 宿主已有 `host.layers`，但缺 `path` 字段 |
| 名称/几何类型/路径等 | 插件把 `host.layers` 排成表格交给 `host.dialog` |

关键认知：**"弹窗"是宿主的职责，不是插件的**。插件只能发请求，界面只能画窗口。两者之间的
桥就是 `Session.pending_dialog`——一个每帧被 UI 取走的槽。

---

## 2. 架构决策

对齐既有模式：`host.notify` → `Session.notices` → 状态栏。新增的 `host.dialog` 走同一条路：

```
插件 host_call("host.dialog", {...})
   → engine::cmd::plugins::Bridge::dialog()   校验 + 裁剪 + 加前缀
   → Session.pending_dialog = Some(PluginDialog)
   → ui_egui::dialogs::show()  每帧 take() 排水
   → ui_egui::dialogs::show_plugin()  渲染成 egui::Window
```

### 三个设计取舍

**① 标题加插件名前缀**（`虾神格式转换: 图层信息`）。
不加的话，任何插件都能开一个叫 "RustGIS - 保存" 的窗口骗用户。这是安全边界，不是装饰。

**② 超限只裁剪，不拒绝**。
若因为标题太长就报错，插件作者要在宿主的限值里猜谜。裁剪则永远能开窗，把能放的放进去。
限值常量（`crates/engine/src/cmd/plugins.rs`）：

| 常量 | 值 | 管什么 |
|---|---|---|
| `DIALOG_TITLE` | 120 | 标题 |
| `DIALOG_HEAD` | 80 | 列头单元格 |
| `DIALOG_COLUMNS` | 12 | 列数 |
| `DIALOG_ROWS` | 500 | 行数 |
| `DIALOG_CELL` | 400 | 单元格 |
| `DIALOG_LINE` | 1000 | 无列头时整行 |
| `DIALOG_NOTE` | 400 | 表格下方备注 |

**③ 只读，不回传**。
`host.dialog` 是展示，不是输入。需要收集参数走 `form`（另一个特性）。关闭窗口不回调插件——
插件拿不到"用户关了"，也就不需要为"用户没关"写状态机。

---

## 3. 实施步骤

### 宿主引擎侧

- `crates/engine/src/lib.rs`：`PluginDialog { title, columns, rows, note }` + `Session.pending_dialog`
- `crates/engine/src/cmd/plugins.rs`：
  - `dialog()` 方法：裁剪、拼装、全空则报错 `host.dialog: the window has nothing to show`
  - `cut()` / `cell()` 辅助（换行制表换空格、非字符串转 JSON）
  - `"host.dialog" => self.dialog(params)` 分支
  - `layers()` 补 `path`：`l.dataset.source.clone().or_else(|| raster.path)`

### 宿主 UI 侧

- `crates/ui-egui/src/dialogs.rs`：`Dialogs.plugin` 字段、`show()` 里 `pending_dialog.take()` 排水、
  `show_plugin()` 渲染（`ScrollArea::both().max_height(420)` + `Grid::striped` + Close 按钮）

### 插件侧

- `plugin.json`：`layer-info` 命令（`icon: "table"`、`params: "{}"`）+ `tab.groups` 第二组「图层」
- `src/lib.rs`：`layer_info()` → `host.layers` → 按 `lang` 生成列头 → `host.dialog` → `{"layers": n}`

几何类型的显示（`host.layers` 的 `geometry` 是 Rust 的 `Debug` 文本）：

| `geometry` | 中文 | 英文 |
|---|---|---|
| `Point` | 点 | Point |
| `Polyline` | 线 | Polyline |
| `Polygon` | 面 | Polygon |
| `None` | 表格 | Table |
| `raster: true` | 栅格 | Raster |
| `path` 为 null | 工程内 | In project |

---

## 4. 踩过的坑（按时间顺序）

### 4.1 `egui_kittest` 的 `click()` 点不掉窗口

冒烟测试最后一步 `Close` 按钮点不动，窗口还在。查了 `egui_kittest-0.36.2/src/node.rs`，
`click()` 是往队列推 `PointerMoved` + `PointerButton` press/release——坐标来自
`self.rect().center()`，是**上一帧** AccessKit 树里的矩形。窗口刚渲染完第一帧，按钮位置
和这一帧的指针命中对不上，事件打空。

改用 `click_accesskit()`：直接发 `AccessKitActionRequest`，绕过坐标，任何可见/不可见控件都能点。

```rust
// 点不动
h.query_all_by_label(close).next().expect("a Close button").click();
// 点得动
h.query_all_by_label(close).next().expect("a Close button").click_accesskit();
```

**教训**：kittest 里凡是"刚出现的控件"或"位置会变的控件"，用 `click_accesskit()`，
不要用坐标版 `click()`。

### 4.2 语言相关的测试断言在中文机上必挂

`Close` 按钮的实际文本是 `tl!("Close")`，中文机上是「关闭」。硬编码 `"Close"` 查不到。
语言无关写法：

```rust
let close = Box::leak(rustgis_ui_egui::i18n::t("Close").to_string().into_boxed_str());
h.query_all_by_label(close).next().expect("a Close button").click_accesskit();
```

### 4.3 `json!([...])` 生成的是 `Value`，不是 `Vec<Value>`

```rust
// 编译错：cannot be built from an iterator over elements of type `Value`
let rows: Vec<Vec<Value>> = list.iter().map(|l| json!([…])).collect();
// 对
let rows: Vec<Vec<Value>> = list.iter().map(|l| vec![…]).collect();
```

`json!([…])` 返回 `Value::Array`，`collect` 收不到 `Vec<Value>`。要么用 `vec![]` 裸装 `Value`，
要么让闭包返回 `Value` 再在外层 `json!`。

### 4.4 `cargo fmt --all` 会改到我没碰的代码

`cargo xtask ci` 第一关 `fmt --check` 挂了，diff 里既有我写的长行，也有工作树里
`form` 特性补丁的既有代码。跑 `cargo fmt --all` 一起格式化。
**教训**：进 CI 前先 `cargo fmt --all`，别只看自己那几行。

### 4.5 clippy 1.96 新 lint 打到既有代码

`crates/data/src/io.rs` 的 `nonminimal_bool`：

```rust
// 挂
let exact = !matches!(v, Value::Int(i) if i.unsigned_abs() > 1 << 53)
    && !(f.ty == FieldType::Double && …);
// 过
let exact = !(matches!(v, Value::Int(i) if i.unsigned_abs() > 1 << 53)
    || f.ty == FieldType::Double && …);
```

按 clippy 提示的等价式改写即可。**教训**：CI 挂了先看是不是既有代码——
`git status` 该文件没改动，就是工具链升级带来的新 lint，最小改写、不扩大范围。

### 4.6 `docs/` 新增文件必须登记进 `docs::PAGES`

上个提交（583dd02）加了 4 个文档没登记，`docs_pages_and_addresses` 测试挂：

```
docs/framework-extraction-feasibility.md is missing from docs::PAGES
```

测试遍历 `docs/**/*.md`，逐个断言 `docs::page(f).is_some()`，再断言数量相等。
修法就是测试注释写的 `add new pages to docs::PAGES`。

**这也是本文档不放 `docs/` 的原因**：放进去会被这个测试抓，还会被 `include_str!`
编译进 assistant 的内置文档。开发实录放仓库根。

### 4.7 中文 Windows 下冒烟套件 5 个"环境性失败"

`cargo xtask ci` 里 `cargo test --workspace` 挂 5 个冒烟测试，全是
`No nodes found matching the query: By { label: "Scroll the tabs right" }` 之类。
根因：`i18n::system_lang()` 里 `cfg!(test)` 只对**单元测试**生效，集成测试（`tests/smoke.rs`）
编译的是库的非 test 版本，于是真去读系统 locale → 中文 → 界面文案被翻译 → 英文标签查不到。

| 挂的测试 | 查的标签 | 中文实际 |
|---|---|---|
| `a_ribbon_wider_…` | `Scroll the ribbon right` | 向右滚动功能区 |
| `a_tab_strip_wider_…` | `Scroll the tabs right` | 向右滚动选项卡 |
| `browse_lists_thousands…` | `5000 plugins` | 5000 个插件 |
| `plugins_can_have_a_tab…` | `More` | 更多 |
| `plugins_install_from_a_file…` | `Install` | 安装 |

`system_lang()` 支持 `RUSTGIS_LANGUAGE` 环境变量覆盖。设成 `en` 后 9/9 全过。

```powershell
$env:RUSTGIS_LANGUAGE="en"; cargo xtask ci
```

**教训**：CI 挂了先分清"我改坏的"和"环境性的"。判断法——只跑那一个测试、
看失败信息是不是"查不到某标签"、设 `RUSTGIS_LANGUAGE=en` 重跑对照。
**本次 5 个失败全是环境性的，与改动无关**，但它们会掩盖真失败，所以最终用英文环境跑通了完整 CI。

### 4.8 其它小坑

- **PowerShell 把 cargo 的 stderr 当错误**：成功时也报 exit code 1。看输出里的
  `Finished` / `test result: ok`，别看 `$LASTEXITCODE`。
- **PATH 里没有 `rg`**：`grep` / `glob` 工具正常，别依赖 shell 的 ripgrep。
- **cargo registry 路径**：本机是 `E:\workspace\rust\.cargo\registry\src\rsproxy.cn-…`，
  不是 `index.crates.io-…`。找 crate 源码时先 `glob` 一遍。
- **测试插件用独立 id**：`eng-dialog` / `ui-dialog`，避免多个测试并发争用全局插件注册表。

---

## 5. 验证

| 层 | 测试 | 覆盖 |
|---|---|---|
| 引擎 | `plugin_dialogs_are_bounded_and_attributed` | 限值裁剪、标题前缀、全空报错、`host.layers` 的 `path` |
| UI | `a_plugin_window_shows_its_table` | 窗口内容、标题前缀、Close 关闭 |
| 插件 | `lists_the_layers_in_a_dialog_end_to_end` | 命令序列 `["host.layers", "host.dialog"]`、6 列 × 2 行表格 |

最终 `cargo xtask ci`（英文环境）：fmt ✓ / clippy ✓ / tests ✓ / layers ✓。

---

## 6. 经验教训（可复用的）

1. **先问"沙箱允许什么"再动手**。插件要弹窗，第一步不是写弹窗代码，是查
   `docs/plugins.md › Safety` 确认沙箱边界。边界决定了架构，架构决定了代码量。
2. **复用既有模式**。`host.dialog` 照抄 `host.notify` → `notices` → 状态栏的排水模式，
   新增代码只是"另一种形状的数据"，没有新概念。
3. **只裁剪，不拒绝**。对外接口（含插件 ABI）宁可给降级结果，也不要给错误——
   调用方无法在运行时修正自己的常量。
4. **安全边界写进数据结构**。`PluginDialog.title` 的注释直接写明"带插件名，防冒充"，
   下一个人就不会"顺手优化掉"。
5. **集成测试不等于单元测试**。`cfg!(test)` 的行为差异会让 `system_lang()` 在两种测试里
   返回不同结果——需要固定环境的测试，自己显式设定，别依赖系统。
6. **CI 挂了先定位，再修**。分清"我改坏的" / "工具链新 lint" / "环境性"三类，
   前两类修，第三类绕过并记录，别混在一起手忙脚乱。
7. **测试断言要语言无关**。UI 测试里查标签一律走 `i18n::t(...)`，
   否则在非英文机器上必挂，而你自己在中文机上永远跑不绿。
8. **新文档要想清楚放哪**。`docs/` 会被 `include_str!` 编译进 assistant、被测试强制登记；
   开发过程类文档放仓库根或插件目录。

---

## 附：改动文件清单

**宿主（未提交）**

- `crates/engine/src/lib.rs`：`PluginDialog` + `Session.pending_dialog`
- `crates/engine/src/cmd/plugins.rs`：`dialog()` / `cut()` / `cell()` / 限值常量 / `host.dialog` 分支 / `layers()` 补 `path`
- `crates/engine/src/tests.rs`：`plugin_dialogs_are_bounded_and_attributed`
- `crates/ui-egui/src/dialogs.rs`：`Dialogs.plugin` / `show()` 排水 / `show_plugin()`
- `crates/ui-egui/tests/smoke.rs`：`a_plugin_window_shows_its_table`
- `crates/data/src/io.rs`：clippy 1.96 `nonminimal_bool` 修复（既有代码）
- `crates/assistant/src/docs.rs`：补登 4 个遗漏文档（上个提交遗留）

**插件**

- `plugins/xiashen-format-converter/plugin.json`：`layer-info` 命令 + 「图层」分组
- `plugins/xiashen-format-converter/src/lib.rs`：`layer_info()` / `geometry_zh()`
- `plugins/xiashen-format-converter/tests/e2e.rs`：Mock 加 `host.layers` / `host.dialog` 分支 + 新断言
- `plugins/xiashen-format-converter/README.md`、`DEVELOPMENT.md`

**文档**

- `docs/plugins.md`：Safety、A dialog of its own、请求表（`host.layers` 加 `path`、新增 `host.dialog`）
- `docs/writing-a-plugin.md`：宿主请求清单加 `host.dialog`
- 本文档

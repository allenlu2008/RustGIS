# 编写你自己的 RustGIS 插件 —— 技术路线与详细步骤

> 目标读者：想给 RustGIS 添加自定义命令/地理处理工具（例如承载特定业务分析逻辑）的开发者。
> 官方参考：`docs/plugins.md`（英文，格式与 ABI 权威说明）；本文是按实施顺序整理的中文操作指南。

---

## 1. 插件是什么

- 一个 **WebAssembly 模块**（`wasm32-unknown-unknown`）+ 一份 **`plugin.json` 清单**，打包为 **`.rgplugin`**（zip）。
- 同一份包在**桌面应用、命令行、浏览器**三端运行，不需要分别构建。
- 运行在 **`wasmi` 解释器沙箱**里：无文件、无网络、无时钟；只能经宿主请求（host request）触达 RustGIS，且每条请求都按清单声明且用户已同意的权限校验。
- 插件可提供两类入口：
  - **命令（command）**：出现在 Plugins 标签页（或插件自有标签页）、命令搜索、脚本与 CLI 中，id 形如 `plugin:<插件id>/<命令id>`；
  - **工具（tool）**：出现在 Geoprocessing 面板的 **Plugins ›** 区，有参数表单、进处理历史、结果可撤销。

官方 SDK 与示例：[opengeos/rustgis-plugins](https://github.com/opengeos/rustgis-plugins)
（`plugins/hello` 最小示例、`plugins/map-explorer` 自有标签页示例）。

---

## 2. 先做技术选型

| 决策点 | 选项 | 建议 |
|---|---|---|
| **承载形态** | 命令 / 工具 / 自有 ribbon 标签页 | 只读统计、通知、跳转 → 命令；对图层做分析并产出新图层 → 工具（参数表单、历史、撤销全免费）；按钮多（>8 组或需菜单）→ 声明 `tab` |
| **实现语言** | Rust（官方 SDK `rustgis-plugin`）/ 任何能产出 wasm 的语言（直接实现 ABI v1） | 首选 Rust + SDK；ABI 只有 3 个 import、2 个 export，其他语言（ AssemblyScript、Go TinyGo、C）也可接 |
| **权限** | 无 / `edit` / `files` / `network` | 最小化：只读分析可不声明；改项目要 `edit`；写用户指定文件要 `files`；加在线瓦片要 `edit`+`network`。安装确认卡会向用户展示权限清单 |
| **分发** | 本地 `.rgplugin` / 文件夹热加载 / 官方注册表 | 开发期用文件夹热加载或 CLI `--plugin`；对外发布走注册表 PR |

**关键认知**：工具（tool）与命令（command）的宿主模型不同——

- **命令**：可主动发 host 请求（`host.layers`、`host.features`、`host.view`、`host.notify`、`host.addLayer`、`map.addTiles`、以及**任意已注册命令 id**，受权限约束）；每次调用有约 2×10⁹ 指令的燃料预算。
- **工具**：宿主把图层参数的要素（GeoJSON FeatureCollection，有选择集则只给选中）随请求的 `layers` 字段一次性给它；工具**只返回结果**（新图层/独立表/选择集/消息），**不发 host 请求**；燃料预算约 4×10¹⁰。结果由宿主像内置工具一样加入项目（一步撤销、失败回滚）。

每次调用都是**全新实例**：模块调用间不保留状态，也不必释放内存。

---

## 3. 环境准备

```sh
# 1) Rust 工具链加 wasm 目标
rustup target add wasm32-unknown-unknown

# 2) 安装 CLI（用于开发期加载与测试；从 RustGIS 仓库构建）
cargo install --path apps/rustgis-cli

# 3) 从官方模板/示例起步（推荐直接复制 hello）
#    https://github.com/opengeos/rustgis-plugins → plugins/hello
```

新插件的 `Cargo.toml` 要点：

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
rustgis-plugin = "<与目标 RustGIS 版本匹配的 SDK 版本>"
```

> 注意：本仓库（RustGIS 主仓）**禁止 `unwrap/expect/panic`**；SDK 用户代码建议同样风格——插件 trap 只会得到错误信息，不会崩掉宿主，但坏味道会传染。

---

## 4. 最小实现：一个命令 + 一个工具

`src/lib.rs`（基于官方 hello 示例）：

```rust
use rustgis_plugin::{Kind, Request, ToolOutput, Value, geojson, host, json};

fn handle(req: Request) -> Result<Value, String> {
    match (req.kind, req.id.as_str()) {
        // 命令：数图层并通知（只读，无需 edit 权限）
        (Kind::Command, "layer-report") => {
            let n = host::layers()?.as_array().map_or(0, Vec::len);
            host::notify(&format!("{n} layers"));
            Ok(json!({"layers": n}))
        }
        // 工具：为每个要素生成外包矩形图层
        (Kind::Tool, "bounding-boxes") => {
            let boxes: Vec<Value> = req.features("input")?.iter().filter_map(|f| {
                let b = geojson::bbox(&geojson::positions(&f["geometry"]))?;
                Some(geojson::feature(
                    geojson::polygon(&[[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]]),
                    json!({}),
                ))
            }).collect();
            Ok(ToolOutput::new().layer("Boxes", geojson::collection(boxes)).into())
        }
        (_, other) => Err(format!("unknown '{other}'")),
    }
}

rustgis_plugin::export!(handle);
```

构建：

```sh
cargo build --release --target wasm32-unknown-unknown
# 产物：target/wasm32-unknown-unknown/release/my_plugin.wasm
```

### 4.1 清单 `plugin.json`

```json
{
  "id": "my-analysis",
  "name": "My Analysis",
  "version": "0.1.0",
  "description": "业务分析：外包矩形与图层报告。",
  "author": "You",
  "homepage": "https://github.com/you/my-plugin",
  "license": "MIT",
  "entry": "plugin.wasm",
  "abi": 1,
  "minRustgisVersion": "0.1.0",
  "permissions": ["edit"],
  "commands": [
    { "id": "layer-report", "label": "Layer\nReport", "help": "悬停提示。", "icon": "stats", "params": "{}" }
  ],
  "tools": [
    {
      "id": "bounding-boxes", "name": "Bounding Boxes", "summary": "为每个要素画外包矩形。",
      "toolbox": "My Analysis",
      "params": [
        { "name": "input", "label": "Input features", "kind": "vectorLayer", "required": true },
        { "name": "whole", "label": "One box for the whole layer", "kind": "bool", "default": false }
      ]
    }
  ],
  "i18n": {
    "zh": {
      "My Analysis": "我的分析",
      "Bounding Boxes": "外包矩形",
      "Input features": "输入要素"
    }
  }
}
```

字段速查：

- `id`：小写字母、数字、连字符；命令/工具最终 id 为 `plugin:my-analysis/layer-report`、`plugin:my-analysis/bounding-boxes`。
- `permissions`：`edit` / `files` / `network`，按需最小声明。
- `commands[].icon`：复用 RustGIS 内置图标名（`stats`、`zoom_full`、`buffer`…），未知名显示拼图占位。
- 工具参数 `kind`：`layer`、`vectorLayer`、`polygonLayer`、`pointLayer`、`field`、`numericField`、`number`、`text`、`choice`（配 `choices`）、`bool` —— Geoprocessing 面板会据此**自动生成表单控件**。
- `i18n`：按界面语言码翻译插件自有文案（含 `\n`）；上限 64 语言、20000 条；`zh-CN` 这类带区域码会匹配到 `zh`。

### 4.2 打包

包名 **`<id>-<version>.rgplugin`**，本质是 zip，`plugin.json` 与 wasm 模块放在根目录（或唯一一个顶层文件夹内）：

```sh
# Windows (PowerShell)
Compress-Archive -Path plugin.json, target/wasm32-unknown-unknown/release/my_plugin.wasm `
  -DestinationPath my-analysis-0.1.0.rgplugin
# 注意：Compress-Archive 会把 .wasm 改名成 .wasm.zip，需用 zip 工具或脚本保证条目名不变；
# 更稳妥的做法是用 `cargo run` 的打包脚本或 zip CLI：
#   zip my-analysis-0.1.0.rgplugin plugin.json plugin.wasm
```

限制：包 ≤ 64 MB，wasm 模块 ≤ 48 MB。

---

## 5. 开发期加载（四种方式）

| 方式 | 命令/操作 | 适用 |
|---|---|---|
| **文件夹热加载（仅桌面）** | Plugin Manager › Settings › 选插件文件夹 | 改完 `cargo build` 即重启加载，最快迭代 |
| **单次 CLI 运行** | `rustgis-cli run --plugin ./my-plugin/ --sample --cmd 'plugin:my-analysis/layer-report'` | 脚本化验证，不污染安装 |
| **安装文件** | Plugin Manager › Settings 安装 `.rgplugin`，或**把文件拖进窗口**；CLI：`rustgis-cli plugins install my-analysis-0.1.0.rgplugin` | 接近最终用户形态 |
| **环境目录** | `$RUSTGIS_PLUGINS_DIR`（或数据目录 `plugins/`） | 放包即随启动加载 |

验证命令是否注册：

```sh
rustgis-cli commands | findstr plugin:my-analysis
rustgis-cli plugins
```

跑一个端到端例子（示例项目 + 插件工具 + 导出地图）：

```sh
rustgis-cli run --plugin ./my-plugin/ --sample \
  --cmd 'gp.run={"tool":"plugin:my-analysis/bounding-boxes","params":{"input":"Countries"}}' \
  --cmd 'share.exportMap={"path":"boxes.png"}'
```

---

## 6. 调试与工程化要点

- **日志**：SDK 的 log（宿主 `rustgis` import 的 `log`）会进应用日志/CLI 输出；命令出错直接 `Err("...")`，宿主显示错误消息，**不会崩**。
- **超时/死循环**：燃料预算耗尽即停；内存上限 512 MB；请求/应答都有大小限制。
- **权限被拒**：会返回错误消息——先检查清单 `permissions` 与安装确认时用户是否同意。
- **工具选中集**：图层有选择集时，工具收到的只是选中要素——这是特性（"对选中要素跑工具"），写文档时提醒用户。
- **命令改项目**：带 `edit` 的插件命令，其改动是**一步撤销**，命令失败整体回滚。
- **无状态**：不要在模块级 static 里存会话数据；要缓存就在宿主侧用命令参数传递。

---

## 7. 测试

- **手动**：第 5 节的 CLI 命令；桌面端 Plugin Manager 安装后从 Plugins 标签页点击。
- **自动化（在你自己的插件仓库）**：把构建出的 `.rgplugin` 作为测试夹具，用
  `rustgis-cli run --plugin … --cmd …` 断言输出文件或退出码；RustGIS 主仓的
  `crates/plugins/src/tests.rs` 展示了宿主侧如何用 `testing` feature 的 WAT 测试模块覆盖
  燃料超限、越界应答、OOM 等异常路径——插件自身也应测错误分支。
- **敌意参数**：学主仓习惯，对你的工具用缺字段/错类型参数跑一遍，确认返回 `Err` 而非行为未定义。

---

## 8. 发布（可选）

1. 在 [opengeos/rustgis-plugins](https://github.com/opengeos/rustgis-plugins) 提 PR，新增
   `registry/<id>.json`：
   - 源码在该仓库：`{"id": "...", "path": "plugins/<dir>", "categories": [...]}`；
   - 源码在你处：`{"id": "...", "source": {"url": "https://…/<id>-<version>.rgplugin", "sha256": "…"}, "categories": [...]}`。
2. CI 构建/下载并校验清单与模块；维护者人工审查。
3. 合并后发布到 [plugins.rustgis.app](https://plugins.rustgis.app)，带哈希固定；用户端
   Browse 安装时按注册表 SHA-256 校验。

也可以自建注册表：Plugin Manager › Settings 填注册表地址（企业内网分发场景）。

---

## 9. 技术路线图（汇总）

```
选型（命令/工具/标签页、权限、语言）
   ↓
复制 plugins/hello → 写 src/lib.rs（rustgis-plugin SDK）
   ↓
写 plugin.json（id、permissions、commands/tools、i18n、可选 tab）
   ↓
cargo build --release --target wasm32-unknown-unknown
   ↓
开发循环：--plugin 文件夹/CLI 加载 → 点击/命令运行 → 看日志与 notify
   ↓
打包 <id>-<version>.rgplugin → 拖入窗口 / plugins install 验证安装路径
   ↓
CLI 端到端测试 + 错误路径测试
   ↓
（可选）注册表 PR → plugins.rustgis.app 发布
```

### 何时不该用插件

- 需要修改渲染器、数据 provider 内核、undo 结构等引擎内部 → 走主仓扩展点或 fork（见
  `docs/framework-extraction-feasibility.md`）。
- 需要同步阻塞长计算且无法拆步 → 插件是单次调用模型，长任务应拆成多次工具运行。
- 需要自带原生代码/系统库 → 沙箱只有 wasm，无法 `dlopen`。

---

## 附：速查

| 事项 | 位置 |
|---|---|
| ABI v1 完整说明 | `docs/plugins.md` › The ABI |
| 宿主请求清单 | `docs/plugins.md` › What a plugin can ask for |
| 权限语义 | `docs/plugins.md` › Safety |
| 宿主侧加载/注册表实现 | `crates/plugins/src/{lib,manifest,package,registry,runtime,store,tools}.rs` |
| CLI 插件命令 | `rustgis-cli plugins --help` |
| 示例插件 | opengeos/rustgis-plugins：`hello`、`map-explorer` |

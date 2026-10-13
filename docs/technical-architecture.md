# 技术架构详解

本文档面向开发者，系统梳理 RustGIS 的技术架构、技术路线、依赖选型、模块划分与具体功能。
项目是一个纯 Rust 编写、分层 Cargo workspace 的跨平台 GIS：可原生运行于 Windows / macOS /
Linux，也可编译为 WebAssembly 在浏览器中运行。

- 版本：`0.1.0`
- 许可证：Apache-2.0
- Rust 版本：`1.90`（edition 2024，resolver 3）
- 仓库：<https://github.com/opengeos/RustGIS>

---

## 1. 技术路线总览

| 维度 | 选择 | 理由 |
| --- | --- | --- |
| 语言 | 纯 Rust | 单一语言贯穿桌面、CLI、浏览器；无 GC、无运行时安装 |
| 目标平台 | 原生三平台 + `wasm32-unknown-unknown` | 同一套代码，桌面与 Web 一致体验 |
| 架构风格 | 分层 crate + 命令驱动 | 层间单向依赖；UI 可替换；引擎可脚本化 |
| UI 框架 | `egui` / `eframe` | 即时模式 GUI，跨平台且便于 wasm 编译 |
| 渲染 | `tiny-skia`（CPU 光栅）+ egui 纹理 | 纯 Rust、无 GPU 依赖，离屏截图可复现 |
| 风险策略 | 绝不崩溃（禁 `unwrap`/`panic`/`unsafe`） | 文件与命令参数视为敌对输入 |
| 扩展机制 | 沙箱化 WebAssembly 插件 + 外部工具集 | 同一插件在桌面、CLI、浏览器运行 |

### 1.1 分层架构

各层只依赖其下方的层，`cargo xtask layers` 强制检查（L6 以下禁止依赖 egui/eframe/rfd）。

| 层 | crate | 职责 |
| --- | --- | --- |
| L0 | `rustgis-geom` | 坐标、范围、Web Mercator、大地测量距离/面积、比例尺 |
| L1 | `rustgis-data` | 要素模型与类型化属性；矢量 I/O；数据提供者；栅格与坐标系 |
| L2 | `rustgis-map` | 图层、符号化、标注、相机、底图、投影、Web 瓦片图层 |
| L2 | `rustgis-analysis` | 地理处理工具（缓冲、裁剪、融合、凸包、连接、统计等） |
| L2 | `rustgis-tiles` | 网络地图瓦片：XYZ/TMS/WMS、Mapbox Vector Tiles、MapLibre 样式子集、瓦片缓存 |
| L3 | `rustgis-render` | tiny-skia 地图渲染器、标注放置、PNG/PDF 导出、字体发现 |
| L3 | `rustgis-plugins` | 沙箱化的 wasm 插件（命令 + 工具）、`.rgplugin` 包、插件注册表 |
| L3 | `rustgis-geolibre` | GeoLibre/Whitebox 工具集（1000+ 栅格/矢量/LiDAR/水文工具） |
| L4 | `rustgis-engine` | `Session`、撤销/重做、命令注册表、功能区特性目录、示例工程 |
| L5 | `rustgis-assistant` | AI 助手：聊天代理，经命令驱动会话，支持 OpenAI/Anthropic/Gemini |
| L6 | `rustgis-ui-egui` | 功能区 egui 前端（可替换） |
| apps | `rustgis` / `rustgis-cli` / `rustgis-web` | 桌面应用、命令行、浏览器 |

### 1.2 一切皆命令（Everything is a command）

每个用户可见动作都是 `crates/engine/src/cmd/*.rs` 里的一个 `CommandSpec`：

```rust
pub struct CommandSpec {
    pub id: &'static str,          // "layer.symbology"
    pub label: &'static str,
    pub location: &'static str,    // "Map › Navigate"
    pub shortcut: &'static str,    // "Mod+S"
    pub params: &'static str,      // 参数说明 JSON，供脚本/Agent 使用
    pub mutates: bool,             // 是否修改工程（决定是否快照撤销）
    pub enabled: fn(&Session) -> Option<&'static str>,
    pub run: fn(&mut Session, &Value) -> CmdResult,
}
```

功能区按钮、快捷键、命令搜索、CLI 全部经 `Session::run` 分发。`run` 会：
1. 对 `mutates` 命令克隆 `Project` 快照；
2. 用 `catch_unwind` 包裹执行（逃逸的 panic 被捕获）；
3. 失败或 panic 时回滚工程与编辑计数；
4. 成功时压入撤销栈（上限 40）并标记 dirty。

工程内共注册 **316 条命令**（引擎侧），另有大量 `ui.*` 纯界面命令位于 `ui-egui`。

---

## 2. 依赖包清单

### 2.1 内部 crate 依赖（workspace.dependencies）

全部通过 `path` 引用，见根 `Cargo.toml:14-25`：

```
rustgis-geom / rustgis-data / rustgis-map / rustgis-analysis /
rustgis-render / rustgis-engine / rustgis-ui-egui /
rustgis-geolibre / rustgis-plugins / rustgis-tiles / rustgis-assistant
```

### 2.2 第三方依赖（按用途）

| 用途 | crate | 版本 | 说明 |
| --- | --- | --- | --- |
| 序列化 | `serde`, `serde_json` | 1 | 结构体 derive + JSON |
| 错误 | `thiserror` | 2 | 类型化错误 |
| 日志 | `log` | 0.4 | 日志门面 |
| 时间 | `web-time` | 1 | 浏览器也可用的时间（std 在 wasm 会 panic） |
| 几何算法 | `geo` | 0.33 | 几何运算（L2 分析） |
| GIS 格式 | `shapefile` 0.9 / `csv` 1 / `flatgeobuf` 6 / `geozero` 0.15 | | 矢量 I/O |
| 列式格式 | `parquet` 60（关 Arrow 特性）、`arrow-array`/`arrow-schema` 60（仅测试） | | GeoParquet 纯 Rust 读写 |
| 栅格 | `tiff` 0.11 | | GeoTIFF 解码/编码 |
| 坐标系 | `proj4rs` 0.2, `crs-definitions` 0.6 | | EPSG 投影，纯 Rust |
| SQLite | `rusqlite` 0.40（`bundled`） | | GeoPackage（仅原生） |
| XML | `roxmltree` 0.21 | | KML/GPX 解析 |
| 压缩 | `miniz_oxide` 0.8, `flate2` 1, `zip` 2, `tar` 0.4 | | Deflate/归档 |
| 图像 | `image` 0.25（png/jpeg/webp）, `tiny-skia` 0.12 | | 编解码 + 光栅化 |
| 文本 | `ab_glyph` 0.2, `epaint_default_fonts` 0.36 | | 字形与默认字体 |
| WebAssembly | `wasmi` 2 | | 纯 Rust wasm 解释器（沙箱） |
| 哈希 | `sha2` 0.10 | | 插件包校验 |
| HTTP | `ureq` 3（rustls） | | 原生网络请求 |
| 地区 | `iana-time-zone` 0.1, `sys-locale` 0.3 | | 世界观自动检测 |
| UI | `egui`/`egui_extras`/`eframe` 0.36, `rfd` 0.17, `egui_kittest` 0.36 | | 前端与测试 |
| 浏览器 | `wasm-bindgen` 0.2, `js-sys` 0.3, `web-sys` 0.3 | | Web 端绑定 |

### 2.3 平台条件依赖（wasm 兼容性关键）

大量依赖用 `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` 隔离，保证 wasm 构建：

- `data`：`rusqlite`（GeoPackage）、`parquet` 的 `zstd` 特性
- `tiles` / `plugins` / `geolibre` / `assistant`：`ureq`
- `ui-egui`：`rfd`
- `rustgis-web`：`eframe`(wgpu)、`rfd`、`wasm-bindgen*`、`web-sys`
- `rustgis`：Windows 下 `embed-resource` 生成图标

### 2.4 workspace 级质量配置

```toml
[workspace.lints.rust]
unsafe_code = "forbid"

[profile.release]
opt-level = 3, lto = "thin", codegen-units = 16
```

各 crate 根文件均带 `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic homage, ...)]`。

---

## 3. 模块分类与具体功能

### 3.1 L0 — `rustgis-geom`（约 430 行）

纯数学，无 I/O。经纬度为 WGS 84 度，投影坐标为 Web Mercator 米（EPSG:3857）。

- `Coord`：二维坐标；`Extent`：轴对齐矩形（含 `EMPTY`/`WORLD_LONLAT`）
- 常量：`EARTH_RADIUS_M`、`MERCATOR_R`、`MAX_LAT`、`PIXEL_M`
- 投影：`lonlat_to_merc` / `merc_to_lonlat` / `extent_to_merc`
- 大地测量：`haversine_m`、`geodesic_length_m`、`geodesic_ring_area_m2`（Chamberlain & Duquette 球面盈余近似）
- 比例尺：`scale_denominator`、`nice_distance`
- 格式化：`format_distance` / `format_area` / `format_lonlat` / `format_dms`
- `triangulate`：多边形三角化

### 3.2 L1 — `rustgis-data`（约 10,170 行，最大数据层）

**要素模型**（`lib.rs`、`geometry.rs`、`value.rs`）
- `Feature { id, geometry, attrs }`、`Dataset { name, geometry_type, fields, features, source, windowed }`
- `Field` / `FieldType` / `Value`（类型化属性）
- `Geometry` / `GeometryType`；读取报告 `ReadReport`（跳过坏要素并计数，绝不 panic）
- 数据集操作：`push`/`push_next`、`ensure_field`、`numeric_range`、`unique_values`、`extent`、`field_index`

**矢量 I/O**（`io.rs`、`interop.rs`、`wkb.rs`、`zip.rs`）
- 读入：GeoJSON、CSV（经纬度）、Shapefile（含 zip）、KML/KMZ（含样式）、GPX
- 输出：GeoJSON、CSV、GeoPackage、GeoParquet、Shapefile、FlatGeobuf、KML/KMZ、GPX、WKT
- `write_atomic` 原子写入

**数据提供者**（`source/`，按文件自带空间索引就地读取，单次视图超过内存的数据集）
- `gpkg`（R-tree）、`fgb`（packed Hilbert R-tree）、`parquet`（row-group bbox）、`gpkg_mem`/`gpkg_blob`
- `gdal`：安装了 GDAL 时转换 GDAL 支持的格式
- `duckdb`：安装了 DuckDB 时执行 DuckDB Spatial 查询
- 统一 `FeatureSource` trait，`Request { extent, limit }` → `Batch`
- `MAX_LIMIT`：wasm 50 万、原生 200 万

**纯 Rust SQLite 读取**（`sqlite.rs`）：浏览器中打开 GeoPackage

**SQL where 子句**（`query.rs`）：`parse` / `eval`，用于定义查询与属性选择

**栅格**（`raster/`）
- `geotiff`（任意像素类型、分块/条带、压缩）、`ascii`（ASCII grid）
- `crs`：EPSG 坐标系统与 `Transformer` 重投影
- `wkt`：WKT 解析

### 3.3 L2 — `rustgis-map`（约 5,347 行）

**地图模型**（`lib.rs`）
- `Map { name, layers, basemap, tiles, camera, bookmarks, projection, ... }`
- `Layer`：数据集 + 渲染器 + 标注 + 可见性/透明度/比例尺范围/定义查询/选择集/遮罩/显示过滤/聚合/挤出/优先级/设计(域、子类型、属性规则、关系、归档)
- 三种特殊图层：栅格层、流式层（大文件按视图就地读取）、图形层
- 投影缓存 `projected_in`（按投影键懒重建）
- 交互命中测试：`hit` / `hits_at` / `hits_in`
- 内置底图 `Basemap`：Paper/Midnight/Ocean/Atlas/Blank

**子模块**：`camera`、`color`(Rgba/Ramp)、`design`、`effects`(混合/显示过滤/聚合/挤出)、`globe`(3D 地球 + 投影)、`graphics`、`layout`(页面布局)、`presentation`、`projection`(Flat/Custom)、`raster`(栅格图层与样式)、`report`、`source`、`symbology`(Symbol/Marker/Renderer/Labeling)、`tiles`、`view`(效果/色觉/裁剪)、`world`

### 3.4 L2 — `rustgis-analysis`（约 2,872 行）

- `ToolSpec`：工具 = id + 工具箱 + 参数规格 + `run` 纯函数；工具基于 `geo`
- `ParamKind`：Layer/PolygonLayer/PointLayer/Field/NumericField/Number/Text/Choice/RasterLayer/VectorLayer/Bool/Path
- `Output`：Layer / Selection / Update / Table / Layers；`OutputLayer`：Vector/Raster/Table
- `Inputs` trait：按名称取数据集、选择集、栅格
- **外部工具注册**：`register_external(key, provider, runner)`，`tools()`/`tool()`/`run()` 统一内置与外部工具
- 子模块：`tools`(缓冲/裁剪/融合/凸包/连接/统计等)、`convert`、`network`(最短路径等)、`neural`(神经网络像素分类)、`rasterops`(视图分析、IDW 等)、`topology`(拓扑规则)

### 3.5 L2 — `rustgis-tiles`（约 5,850 行）

瓦片地址与数学、后台抓取缓存、绘制到 pixmap（Web Mercator）。

- `address`（XYZ/TMS 瓦片地址）、`source`(Kind/Spec)、`cache`(后台抓取 + 代际)、`draw`(不阻塞网络绘制)
- `mvt`(Mapbox Vector Tiles)、`pmtiles`(PMTiles 归档)、`style`(MapLibre 样式子集)、`sprite`(精灵图)、`template`(URL 模板)
- 缺瓦片时用其它缩放级别的瓦片顶替，并告知是否有更多在路上

### 3.6 L3 — `rustgis-render`（约 4,261 行）

- `render`：把 `Map`（底图、经纬网、图层、选择集）绘制到预乘 RGBA 的 `tiny_skia::Pixmap`
- 标注：贪心无碰撞放置（按优先级，最多 5000 候选），`PlacedLabel`/`PlacedIcon`
- 3D 地球：地平线裁剪、大圆边加密、大气光晕、球体明暗
- `render_png` / `render_complete`（等待瓦片到达的分辨率优先渲染）
- 子模块：`effects`(混合/聚合/挤出)、`fonts`(CJK 字体发现与 URL)、`layout`、`logo`、`pdf`(PDF 图像流 Deflate)、`picture`、`post`(后期效果/色觉模拟)、`raster`(栅格绘制)、`report`、`text`(文本度量)、`tiles`
- 内置 Natural Earth 1:110m 示例数据（build.rs 拉取），支持世界观切换（默认/中国）

### 3.7 L3 — `rustgis-plugins`（约 1,884 行）

- 插件 = WebAssembly 模块 + `plugin.json` 清单，打包为 `.rgplugin`（zip）
- 沙箱 `runtime`（`wasmi`，带 fuel 与内存限制）：不能碰文件/网络，只能经权限校验的请求访问会话
- `manifest`（命令/工具/选项卡/权限/菜单/i18n）、`package`、`registry`(插件注册表 + 黑名单)、`store`(下载)、`tools`(把插件工具接入 analysis)
- 同一模块在桌面、CLI、浏览器运行；插件命令 id 形如 `plugin:<plugin>/<command>`

### 3.8 L3 — `rustgis-geolibre`（约 1,245 行）

- 把 GeoLibre/Whitebox 工具集作为地理处理工具注册到 `rustgis-analysis`
- 原生：调用可选的 `geolibre` runner 程序（`install.rs` 一键下载安装，SHA-256 校验）
- 浏览器：由 web 应用异步运行 geolibre-rust 的 WASI runner（`external_start` → `Services::geolibre_run` → `gp.finish`）
- `Job`：暂存输入（栅格用原文件/嵌入字节，矢量转 GeoJSON 到米制坐标系），输出回读为经纬度图层

### 3.9 L4 — `rustgis-engine`（约 16,441 行）

- `Session`：工程、路径、dirty、命令注册表、revision、活动地图/图层/工具、通知、撤销/重做、范围历史、剪贴板、待处理异步运行
- `Project`（`project.rs`）：多地图、布局、报表、演示、笔记本、GP 历史
- `Registry`：`cmd::all()` 汇总 25 个命令模块
- `catalog.rs`：316 条功能区特性清单（tab/group/name → command），`rustgis-cli parity` 输出覆盖率，测试强制覆盖率下限只升不降
- `sample.rs`：World Atlas 示例工程、世界观切换
- 命令模块（`cmd/`）：`project`、`navigate`、`tiles`、`layer`、`raster`、`select`、`edit`、`clipboard`、`labels`、`graphics`、`layout`、`report`、`presentation`、`notebook`、`items`、`design`、`workflows`、`effects`、`imagery`、`deeplearning`、`view`、`workspace`、`gp`、`share`、`plugins`、`sharing`

### 3.10 L5 — `rustgis-assistant`（约 2,791 行）

- `provider`：OpenAI / Anthropic / Gemini 三种线格式的纯请求/响应数据（`Api`/`Config`/`PRESETS`）
- `tools`：模型可调用的工具，标记哪些需要用户批准（文件/网络/插件命令等待批准）
- `agent`：无 I/O 的对话循环（`Agent`/`Entry`/`Step`），桌面线程、浏览器 `fetch`、CLI、测试共用
- `http`：桌面与 CLI 的阻塞传输
- `mcp`：同一工具集作为 MCP 服务器（`rustgis-cli mcp`），供 Claude Code / Cursor / VS Code 等使用
- `docs`：内置文档供 MCP 客户端检索

### 3.11 L6 — `rustgis-ui-egui`（约 16,341 行）

薄前端：读取 `Session` 状态、按 id 运行命令；`ui.*` 命令在此处理。

- `ribbon.rs`：功能区与上下文选项卡；`panes.rs`：内容/目录/地理处理面板；`mapview.rs`：地图视图与弹出
- `table.rs`：属性表；`dialogs.rs`：文件对话框；`backstage.rs`：工程后台
- `analysispane`(工作流/ModelBuilder)、`assistantpane`(AI 助手)、`editpanes`(编辑表单)、`layoutview`(布局)、`reportpane`(报表)、`docpanes`(文档/笔记本)、`pluginsui`(插件管理器)、`workspacepanes`
- `i18n/`：15 种语言 `.tsv` 目录（az/de/es/fr/id/it/ja/ko/nl/pt/ru/tr/vi/zh）+ `tl!` 宏；CJK 字体按需安装/下载
- `Services`：平台钩子（桌面用原生对话框；Web 用异步文件选择器、下载、fetch、GeoLibre WASI runner）
- `icons.rs`：图标全部代码绘制（无第三方图像资产）

### 3.12 应用层（apps）

| 应用 | 说明 |
| --- | --- |
| `rustgis` | 桌面应用（eframe + wgpu），默认打开 World Atlas 示例 |
| `rustgis-cli` | 命令行：`commands`/`tools`/`info`/`render`/`run`/`ask`/`mcp`/`parity`/`plugins`；每个功能区动作都有 id |
| `rustgis-web` | 浏览器（WebAssembly，WebGPU 回退 WebGL2），含异步 GeoLibre runner |

---

## 4. 跨切面机制

### 4.1 绝不崩溃

- 非测试代码禁用 `unwrap`/`expect`/`panic!`/`unreachable!`/`todo!`/`unsafe`
- 文件与命令参数视为敌对输入：用 `get()` 而非索引，钳制尺寸，限制递归
- `Session::run` 与 `analysis::run` 用 `catch_unwind` 兜底逃逸的 panic
- `engine/src/tests.rs::hostile_params_never_panic` 与 analysis 对应测试用垃圾参数跑遍所有命令/工具；每个崩溃修复都带回归测试

### 4.2 wasm 兼容（不可破坏）

- `apps/` 以下必须能构建 `wasm32-unknown-unknown`（CI 跑 `cargo build --target wasm32-unknown-unknown -p rustgis-web`）
- 用 `web-time` 而非 `std::time`；文件访问在 Web 端经 `Services`
- 网络抓取：原生用 `ureq` 工作线程；浏览器用 `fetch`

### 4.3 国际化

- 用户可见文案用 `tl!("…")` 包裹；格式化消息用 `i18n::fmt("… {name} …", &[("name", …)])`
- 新增字符串需加入 `crates/ui-egui/src/i18n/*.tsv` 全部目录
- 命令 id、数据与 CLI 不翻译

### 4.4 可复现截图

所有 README 截图由 `cargo run -p rustgis-ui-egui --example ui_shot -- script.jsonl` 离屏渲染。

---

## 5. 质量门禁与工程化

```sh
cargo xtask ci   # fmt + clippy -D warnings + tests + layering
```

- `xtask` 实现 CI 子命令：格式化、clippy、测试、分层检查
- `egui_kittest` 做 UI 快照测试（`tests/smoke.rs`）
- 覆盖率测试以功能区清单为基准，强制只升不降
- GitHub Actions：`ci.yml`、`release.yml`、`docs.yml`、`web-deploy.yml`、`package-managers.yml` 等
- 资产政策：无第三方图标/图片/字体/帮助文本；`ATTRIBUTION.md` 逐项登记；截图不提交仓库，托管在 `rustgis-assets` 并由文档按 URL 引用

---

## 6. 数据格式支持速览

| 方向 | 格式 |
| --- | --- |
| 矢量读入 | GeoJSON、CSV、Shapefile（含 zip）、GeoPackage、GeoParquet、FlatGeobuf、KML/KMZ（含样式）、GPX、GDAL 可读格式（安装 GDAL 时）、DuckDB 查询（安装 DuckDB 时）、PMTiles |
| 矢量写出 | GeoJSON、CSV、GeoPackage、GeoParquet、Shapefile、FlatGeobuf、KML/KMZ、GPX、WKT |
| 栅格 | GeoTIFF / COG（任意像素类型、分块/条带、压缩）、ASCII grid；任意 EPSG 重投影 |
| 工程/资源 | `.rgis` 工程、`.rgismap` 地图、`.rgislyr` 图层、`.rgisstyle` 样式、`.rgistemplate` 模板、`.rgistask` 任务、`.rgisreport` 报表、`.rgispres` 演示、`.rgisnb` 笔记本、`.rgislyr` 包、`.rgplugin` 插件、`.rgispkg` 工程包 |

---

## 7. 关键设计取舍

1. **CPU 渲染而非 GPU 管线**：`tiny-skia` 保证桌面与浏览器一致、离屏截图可复现；raster 高 DPI 时按逻辑像素分辨率计算以提速。
2. **流式读取**：GeoPackage/FlatGeobuf/GeoParquet 按空间索引就地按视图读取，支持远超内存的数据集；编辑前需 `layer.loadAll`。
3. **命令即 API**：CLI、Agent、插件、功能区共用一套命令，天然可脚本化、可测试、可撤销。
4. **沙箱插件**：`wasmi` 解释器 + 权限声明，跨三端一致，且不牺牲安全性。
5. **外部工具进程化**：GDAL/DuckDB/GeoLibre 通过可选程序调用，避免把庞大 C 依赖编入二进制，保持 wasm 可编译。
# RustGIS 框架提取可行性研究

> 目标：评估将 RustGIS 框架提取出来、仅保留 GIS 基础功能、改造成特定业务分析软件的可行性。
>
> 结论：**可行，且架构为此设计**（Apache-2.0，允许闭源商用改造）。主要成本在产品裁剪决策与
> catalog/测试同步维护，而非底层改造。建议先走插件路线验证业务，确需白标再 fork。

---

## 1. 架构前提：为什么"提取"在这套代码里成本低

RustGIS 的分层与约定天然支持裁剪（见 `AGENTS.md`）：

| 前提 | 对提取的意义 |
|---|---|
| **分层清晰**（L0 geom → L4 engine → L6 ui-egui） | 保留 L0–L4 + L6 即得完整 GIS 底座；`layers` xtask 强制依赖方向，拆层不会牵一发动全身 |
| **一切皆命令**（`CommandSpec`，UI 只经 `Session::run` 分发） | 砍功能 = 删命令注册；加业务 = 注册新命令，UI 表单按 `ParamKind` 自动生成 |
| **`Services` 平台钩子**（`ui-egui/src/lib.rs:62`） | 桌面/Web 差异被隔离在钩子后；只做桌面可整体不解锁 wasm 约束 |
| **Ribbon 按标签页函数组织**（`ribbon.rs` 各 `*_tab`） | 删标签 = 删函数 + 改 `TABS`，机械劳动 |
| **插件体系**（`crates/plugins`，WASM 沙箱） | 业务逻辑可不 fork 实现（路线 A，见 §3） |
| **覆盖率清单 + 敌意参数测试**（`catalog.rs`、`hostile_params_never_panic`） | 砍命令有自动检查兜底；测试表是资产不是负担 |
| **i18n 外置**（`tl!` + `*.tsv`） | 品牌/语言替换无需改代码结构 |

---

## 2. 许可与合规

- **许可证**：Apache-2.0 —— 允许修改、闭源分发、商用，仅需保留版权与许可声明（NOTICE 类义务）。
- **资产**：项目自身无第三方图标/字体/样式（图标在 `icons.rs` 代码绘制）；样例数据来自 Natural Earth，
  其归属行在 `ATTRIBUTION.md` ——**若成品仍分发样例数据，该文件的归属不能删**。
- **依赖**：改造前用 `cargo deny` / 许可证审计复核依赖树（尤其渲染/地理计算相关 crate）。

---

## 3. 两条路线

### 路线 A：不剥离，用插件体系承载业务（成本最低，推荐先验证）

业务分析逻辑 = 一个或多个 `.rgplugin` 沙箱 WASM 插件：

- 插件可自带**命令和工具**，自动出现在工具箱与命令搜索中；
- 可声明**自有 ribbon 标签页**（`ribbon.rs` 的 `plugin:<id>` 机制）；
- 权限确认、注册表、安装管理 UI 全部现成（`pluginsui.rs`）。

| 优势 | 局限 |
|---|---|
| 零 fork，上游更新照常合并 | 插件不能深度替换界面：不能砍掉无关 ribbon、不能换品牌壳 |
| GIS 底座不用自己维护 | 适合"GIS 软件 + 业务模块"形态，不适合白标独立产品 |
| 两周量级可验证业务想法 | 复杂业务若需改引擎内部（新数据 provider 等）仍受限 |

**适用**：业务表达为"命令 + 参数表单"即可；产品形态允许保留 RustGIS 外观。

### 路线 B：提取框架，改白标业务软件

适合"以业务命名的独立软件"。详见 §4（保留/砍掉清单）与 §5（工作量）。

---

## 4. 路线 B：保留与砍掉清单

### 4.1 保留（GIS 基础功能）

| 层 | Crate | 职责 |
|---|---|---|
| L0 | `geom` | 坐标、范围、投影、量测 |
| L1 | `data` | 要素/属性、GeoJSON/CSV/Shp/KML/GPX I/O、栅格读写、空间 SQL |
| L2 | `map` | 图层、符号化、标注、相机 |
| L3 | `render` | tiny-skia 渲染、PNG/PDF 导出 |
| L4 | `engine` | `Session`、命令注册表、撤销、目录——**必须保留**，UI 只依赖它 |
| L6 | `ui-egui` | 换皮主战场 |

### 4.2 砍掉（按需）

| 目标 | 方式 |
|---|---|
| 通用地理处理工具箱 | 删 `analysis` 中不需要的工具；或整体删 `analysis`，保留少数（buffer/clip 等） |
| GeoLibre 套件 | cargo feature `geolibre` 关闭即可（可选依赖） |
| AI 助手 | 删 `assistant` crate 依赖与 `assistantpane.rs`、对应 `RightPane` 变体 |
| 在线底图/瓦片 | 删 `tiles` 依赖与 `map.basemap`/`addTiles` 相关命令、对话框 |
| Web/浏览器版 | 不移植 `apps/rustgis-web`；wasm 兼容约束解除（`web_time` 可留） |
| 分享/联网功能 | 删 `ui.share`、Shares 面板、插件注册表浏览 |

### 4.3 明确不要做的事

- **不要重命名 crate**（保留 `rustgis_engine` 等依赖名），只换 app 壳与入口 —— 否则每次上游合并都是大冲突；
- 不要在 L0–L4 引入 UI 依赖（`cargo xtask layers` 会拦截）。

---

## 5. 路线 B 工作量分解

| 事项 | 说明 | 量级 |
|---|---|---|
| Ribbon 重排 | `ribbon.rs`（3600+ 行）按标签页函数组织：删 `*_tab` 函数、改 `TABS` 常量；业务工具照抄 `GpForm` 模式（注册 `CommandSpec` → 表单自动生成） | 中，机械劳动 |
| Catalog 同步 | `engine/src/catalog.rs` 的 ribbon 覆盖率清单有"只能上升"的测试，砍功能须同步修剪 | 小-中 |
| 敌意参数测试 | `hostile_params_never_panic` 表随命令增删维护 | 小，长期资产 |
| 品牌替换 | `icons.rs` logo、`theme.rs` `BRAND` 色、`chrome.rs` About/链接、文档 | 小 |
| i18n | 沿用 `tl!` 体系，业务新词条补进各 `*.tsv` | 小 |
| 业务命令开发 | 每个业务分析 = 一个 `CommandSpec` + `run` 实现；复杂表单参考 `analysispane.rs` 28 节模式 | 主要成本，取决于业务 |
| 新数据格式（如需） | 照 `data/source` 里 GeoParquet/FlatGeobuf 的 provider 模式扩展 | 按需 |

**建议节奏**：复制 `apps/rustgis` 壳 → 删 ribbon 标签与 engine 命令直到 `cargo xtask ci` 绿 → 往空壳注册业务命令。

---

## 6. 风险

| 风险 | 缓解 |
|---|---|
| fork 后维护承诺（安全修复、格式适配如 GDAL/SQLite 外部程序） | 尽量少改底层；业务集中在 engine 命令层与 ui 层 |
| 上游演进导致合并冲突 | 不重命名、不移动保留 crate；改动集中在新文件与 app 壳 |
| 裁剪过度导致引擎内部死代码 | 分阶段砍：先 UI 标签，再命令，最后才考虑删 crate |
| 依赖许可证问题 | 改造前做一次依赖许可证审计 |
| 样例数据归属 | 保留 `ATTRIBUTION.md` 对应行或移除样例数据 |

---

## 7. 决策序（建议）

1. 业务逻辑能否表达为"命令 + 参数表单"？
   - 能 → **路线 A（插件）**，两周量级验证业务价值；
2. 是否必须白标 / 必须砍掉无关 UI？
   - 是 → **路线 B**，按 §5 最小切口起步；
3. 平台选择：桌面优先（`rfd` 原生对话框、打印、文件夹连接都在桌面侧），Web 按需再移植（`Services` 抽象使移植成本可控）。

---

## 附：相关文档

- `docs/ui-egui-modules.md` —— ui-egui 各模块功能注释（裁剪 UI 时的逐模块参考）
- `AGENTS.md` —— 分层、质量门禁（`cargo xtask ci`）、"一切皆命令"等架构约定

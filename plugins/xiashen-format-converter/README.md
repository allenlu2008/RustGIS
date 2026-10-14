# 虾神格式转换（xiashen-format-converter）

RustGIS 桌面端插件：在**独立选项卡**上点一个按钮，弹出对话框选择本机的 Shapefile
（.shp），执行后转换为 **GeoJSON** 保存到 shp 同目录，并在状态栏报告结果。

| 需求 | 实现 |
|---|---|
| ① 独立选项卡，放在界面最后面 | `plugin.json` 的 `tab` 字段。插件选项卡自动排在内建选项卡（Map、Insert、…）之后 |
| ② 按钮 → 弹出分析窗口：文件选择框＋选择按钮＋执行按钮 | 按钮是带 `form` 的命令。点击后宿主弹出对话框：路径输入框（文件选择框）、**浏览**按钮（系统文件选择器，过滤 `*.shp`）、**执行**按钮。*需要应用可选的宿主补丁，见下* |
| ③ 选择本机 shp 文件 | 浏览按钮调起系统文件选择器，仅列出 `.shp`；也可手动粘贴路径 |
| ④ 执行 = 另存为其他格式 | `layer.addData`（读入 shp）→ `share.exportLayer`（写 `<同名>.geojson`）→ `layer.remove`（清理临时图层）→ 状态栏通知 |
| ⑤ 图层信息按钮 → 弹出窗口 | `layer-info` 命令：`host.layers`（读内容列表）→ 排成表格 → `host.dialog`（宿主弹出只读窗口：名称、几何类型、路径、要素数、字段数、可见） |
| ⑥ 移除所有图层按钮 | `clear-layers` 命令：`host.layers`（先取快照）→ 逐个 `layer.remove`（按 id）→ 状态栏通知；整个命令**一步撤销**，中途失败宿主整体回滚 |

## 目录结构

```
xiashen-format-converter/
├── plugin.json            # 清单：选项卡、命令、form 对话框、权限
├── Cargo.toml             # 独立 crate（不属于 RustGIS 根 workspace）
├── src/lib.rs             # 插件逻辑 + 按 ABI v1 手写的宿主胶水（无需外部 SDK）
├── tests/e2e.rs           # 端到端测试：用宿主的 rustgis-plugins 真实加载并运行
├── build.ps1              # 构建 plugin.wasm 并打包 .rgplugin
├── patches/
│   └── 0001-plugin-command-forms.patch   # 可选：宿主“命令表单对话框”补丁
├── README.md
└── DEVELOPMENT.md         # 完整开发记录：调研、设计决策、踩坑、验证，供后期参考
```

## 构建与打包

```powershell
# 需要 rustup target add wasm32-unknown-unknown
.\build.ps1                 # cargo build --release --target wasm32-unknown-unknown
                            # → 复制 plugin.wasm → 打包 xiashen-format-converter-0.1.0.rgplugin
```

测试（先跑 build.ps1，否则会跳过）：

```powershell
cargo test --manifest-path .\Cargo.toml
```

## 安装（三选一）

1. **图形界面**：`Plugins › Plugin Manager › Settings › Install…` 选
   `xiashen-format-converter-0.1.0.rgplugin`；也可以直接把文件拖到窗口上。
2. **命令行**：`rustgis-cli plugins install plugins\xiashen-format-converter\xiashen-format-converter-0.1.0.rgplugin`
3. **开发模式（文件夹加载）**：`rustgis-cli run --plugin .\plugins\xiashen-format-converter\ …`，
   或在 Plugin Manager 的 Settings 里选择插件文件夹。

安装时插件会申请 `edit` 与 `files` 两项权限：`edit` 用于把 shp 临时读入项目（一步撤销），
`files` 用于读写本机文件（读 shp、写 GeoJSON）。

## 图层信息：宿主代弹的窗口

“图层”分组里的**图层信息**按钮不需要任何补丁：插件发 `host.layers` 拿到内容列表，把每个
图层排成一行（名称、几何类型、路径、要素数、字段数、可见），再发 `host.dialog` 交给宿主
弹出只读窗口。宿主会把插件名放进标题（`虾神格式转换: 图层信息`），窗口因此不能冒充
RustGIS 自己的；标题、列头、行与单元格有长度上限（超出截断，不拒绝），窗口可滚动。
详见 `docs/plugins.md` › A dialog of its own。

## 移除所有图层：快照后逐个删

“图层”分组里的**移除所有图层**按钮把当前工程内容列表里的图层全部移除：

1. `host.layers` 读一次内容列表，**先取快照**（删除过程中列表会变，但图层 id 稳定）；
2. 逐个发 `layer.remove`，参数用 `{"layer": <id>}`（宿主支持 id 或名称），栅格图层同样适用；
3. 全部删完后 `host.notify` 在状态栏报告“已从当前工程移除全部 N 个图层”，
   命令返回 `{"removed": N}`。

整个命令是**一个撤销步骤**（一步 `Ctrl+Z` 全部恢复）；任何一步失败，宿主会把整个命令
**回滚**，不会留下删了一半的工程。列表为空时不碰宿主命令，直接给出可读的错误提示。
权限用的是已声明的 `edit`（与“格式转换”共用），无需新增。

## 关于“弹窗”：可选的宿主补丁

RustGIS 的插件沙箱**不允许插件自己弹窗口或调起系统文件选择器**（见 `docs/plugins.md` ›
Safety）。本插件的 `plugin.json` 用了一个扩展字段 `commands[].form` 声明对话框，需要宿主
支持才能弹窗。补丁以 diff 形式随插件提供，**不改动源工程的任何文件，是否应用由你决定**：

```powershell
# 应用（在 RustGIS 仓库根目录）
git apply plugins/xiashen-format-converter/patches/0001-plugin-command-forms.patch

# 验证
cargo test -p rustgis-plugins
cargo clippy --no-deps -p rustgis-plugins -p rustgis-ui-egui --all-targets -- -D warnings

# 回退
git apply -R plugins/xiashen-format-converter/patches/0001-plugin-command-forms.patch
```

补丁内容（约 20 个文件，含文档）：

- `crates/plugins/src/manifest.rs`：`commands[].form` 字段与校验（`path` / `text` / `number`，
  `accept` 过滤扩展名，`required` 必填）。
- `crates/ui-egui/src/lib.rs`：点击带 form 的插件命令时打开对话框（而不是直接运行）。
- `crates/ui-egui/src/dialogs.rs`：对话框渲染——输入框、**浏览**（系统文件选择器）、
  **执行**（把字段作为参数提交回命令，带 `_form` 标记）。
- `crates/ui-egui/src/i18n/*.tsv`：新增 `Execute`（执行）与 `{label} is required` 的
  14 种语言翻译。
- `crates/plugins/src/tests.rs`、`docs/plugins.md`：form 校验测试与文档。

**未应用补丁时**：插件仍可正常安装，选项卡和按钮照常显示；点击按钮会直接运行命令，
命令因拿不到文件路径而返回一条中文错误提示（状态栏可见），不会崩溃。

## 行为细节

- 输出路径：`C:\data\demo.shp` → `C:\data\demo.geojson`（同目录同名，扩展名替换）。
- 读入的临时图层在导出后会被移除，地图保持原样；整个命令是**一个撤销步骤**
  （失败时宿主自动回滚，不残留半成品图层）。
- 只接受 `.shp`（大小写不敏感）；同目录需要 `.dbf` / `.shx` 等配套文件（由宿主读取）。
- 界面为中文环境时插件消息为中文，其他语言为英文（跟随 `lang` 参数）。

## 技术备注

- crates.io 上没有 `rustgis-plugin` SDK crate，本插件按 `docs/plugins.md` 的 ABI（版本 1）
  手写胶水：导出 `memory` / `rustgis_alloc` / `rustgis_call`，导入 `rustgis.call` /
  `rustgis.result` / `rustgis.log`，JSON 收发。非 wasm 目标下 `src/lib.rs` 为空，
  因此 `tests/e2e.rs` 可以直接在宿主编译平台运行。
- 打包没有用 `Compress-Archive`：PowerShell 5.1 会用反斜杠写 zip 条目名，而 RustGIS 的
  包读取器按正斜杠匹配 `plugin.json`；`build.ps1` 用 .NET `ZipArchive` 显式控制条目名。
- `plugin.wasm` 约 77 KB，`.rgplugin` 约 34 KB（远低于 64 MB 上限）。

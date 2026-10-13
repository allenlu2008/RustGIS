# RustGIS ui-egui 模块功能注释说明

> 对 `crates/ui-egui` 各源码文件逐模块、逐关键代码的功能说明。UI 层是"薄"的：只读取
> `Session` 状态，一切业务动作都通过命令 id（`RustGisApp::run`）转发给引擎；本层的
> `ui.*` 命令只管理界面自身（面板、对话框、主题、偏好）。
>
> 行号基于写作时的源码，可能随代码演进略有偏移，以符号名为准。

---

## 1. `lib.rs` — 应用根与命令分发

文件主旨：L6 层桌面/浏览器通用前端。以"带上下文选项卡的功能区"为核心布局，配合左右停靠面板、地图视图、属性表与状态栏；同时声明 clippy 严格禁令（禁止 `unwrap/expect/panic/…`）。

### 1.1 核心类型

| 类型 | 位置 | 说明 |
|---|---|---|
| `Inbox` | `lib.rs:37` | `Arc<Mutex<Vec<(文件名, 字节)>>>`：异步文件选择器/拖放送达的文件信箱，每帧被排空 |
| `Slot<T>` | `lib.rs:40` | `Arc<Mutex<Option<T>>>`：异步服务（GeoLibre、插件下载、AI HTTP）的结果槽，每帧轮询 |
| `Start` | `lib.rs:43` | 外壳对启动偏好的单次覆盖：`blank` / `projection` / `extent`（Web 的 `?blank` 等 URL 参数用） |
| `Services` | `lib.rs:62` | **平台钩子集合**。桌面端留空走原生对话框；Web 端注入 `open_async`（浏览器文件选择器）、`download`（Blob 下载）、`fetch_font`、`fetch`、`http`、`geolibre_*` 等。判断"是否 Web"的惯用法是检查 `services.download.is_some()` |
| `LeftPane` | `lib.rs:136` | 左面板：`Contents`（绘制顺序）、`Catalog`（目录） |
| `RightPane` | `lib.rs:143` | 右面板 18 种：Geoprocessing / Symbology / History / Details / Editing / Design / Workflows / Script / Tasks / Jobs / Profile / Diagnostics / Infographics / Report / Presentation / Notebook / Assistant / Shares |
| `UiState` | `lib.rs:165` | 纯界面状态：当前标签页、backstage、左右面板、深浅主题、语言/世界观偏好、功能区最小化、命令搜索、隐藏标签、快速访问、面板布局集、`pending_open`（Web 选中文件待执行的命令）、帧末副作用队列 `copy_out`/`copy_image`/`open_url` |
| `RustGisApp` | `lib.rs:257` | 应用根：`session`（引擎会话）+ `ui` + 各面板状态（`map`/`dialogs`/`gp`/`table`/`layout`/`forms`/`workflow`/`ws`/`plugins`/`reports`/`docs`/`assistant`）+ `services` + 字体安装状态机字段 |

### 1.2 生命周期

```
with_sample()/new(session)      建会话与默认 UI 状态
  → with_services(services)     注入平台钩子；读回持久化偏好（语言/世界观/启动项/功能区/插件）
  → start(&Start)               按偏好+覆盖建启动项目；clear_history() 使启动不可撤销
  → 每帧: logic(ctx) → ui(ui)   logic 做不绘制的工作，ui 绘制
```

- **`logic`（`lib.rs:1407`）**：轮询异步任务（`poll_geolibre`/`pluginsui::poll`/`assistantpane::poll`）；消费 `font_inbox` 下载字体、`inbox` 文件 → `open_bytes`；桌面另读 egui `dropped_files`；语言切换守卫（字体就绪才切 `shown_lang`，避免豆腐块）；主题变化时 `theme::apply`。
- **`ui`（`lib.rs:1510`）**：前两帧只等字体；正常帧依次画 `chrome::header` → `ribbon::show` → `chrome::status_bar` → `panes::show` → `table::show` → `mapview::show`（或 `backstage::show`），再叠 `dialogs`/`pluginsui`/命令搜索/About/notices；帧末兑现 `copy_out`/`copy_image`/`open_url`，更新窗口标题。

### 1.3 关键方法

| 方法 | 位置 | 说明 |
|---|---|---|
| `with_services` | `lib.rs:334` | 注入钩子、加载偏好、恢复 web 插件与 GeoLibre 运行器 |
| `start` | `lib.rs:628` | 启动项目/投影/范围；失败仅记录不中断 |
| `lang` / `chosen_lang` / `can_draw` | `lib.rs:379-392` | 实际绘制语言 vs 用户选择语言的分离：CJK 字体未就绪时保持旧语言绘制 |
| `ensure_script_font` | `lib.rs:432` | 当前文字无字体时：桌面用已装 CJK 字面，Web 经 `services.fetch_font` 下载 |
| `open_bytes` | `lib.rs:484` | **Web 打开文件的总入口**：按扩展名分派——`.rgplugin`→插件确认；`.rgismap` 等项目项→对应命令（base64 `data` 参数）；栅格/多图层/数据源→`layer.addDataBytes`；文本→`layer.addDataText`/`project.openText` |
| `run` | `lib.rs:663` | 命令总入口：`ui.*` → `run_ui`，其余 → `session.run` |
| `run_ui` | `lib.rs:674-1404` | `ui.*` 命令巨型分发：面板开关、偏好（`ui.language`/`ui.worldview`/`ui.startup`）、打开/保存（区分 Web 下载与桌面 `rfd` 对话框）、导出（地图/要素/栅格/版面/报表/演示）、共享（`ui.share`）、插件、识别、剪贴板等 |

### 1.4 横切逻辑

- **i18n**：每帧 `i18n::set_current(lang)`；文案一律 `tl!("…")` 或 `i18n::fmt`；引擎侧同步 `plugins::set_language`。
- **字体（CJK 核心难点）**：`fonts_dirty → install_fonts → fonts_pending → can_draw → shown_lang` 状态机；当前区域的 CJK 字面排在界面字体之后使其字形优先，其余作兜底。
- **主题**：`applied_dark != ui.dark` 时构造 `Tokens::dark()/light()` 并 `theme::apply`。
- **防崩溃**：绕过 `Session::run` 的直接调用（版面/报告/地图导出等）都用 `catch_unwind` 复刻引擎的最后防线。

---

## 2. `chrome.rs` — 窗口外壳

文件主旨：标题栏、状态栏、全局快捷键、命令搜索（命令面板）、About 与通知 toast。

| 函数 | 位置 | 说明 |
|---|---|---|
| `header` | `chrome.rs:12` | 顶部 34px 标题栏：自绘 logo、快速访问工具栏（新建/打开/保存/撤销/重做 + 用户自定义项）、上下文选项卡组色带、自适应命令搜索框（空间不足退化为圆形图标）、居中标题（工程名 + 地图名，放不下逐级丢弃绝不重叠） |
| `status_bar` | `chrome.rs:168` | 底部 26px 状态栏：比例尺下拉（`map.setScale`）、指针经纬度（十进制/度分秒切换）、投影标签（点击开坐标系对话框）、选中计数 + 清除按钮、渲染耗时、最新通知 |
| `shortcuts` | `chrome.rs:264` | 全局快捷键：Ctrl+Z/Y、S/Shift+S、O、N、K、F1、T、+/−；非输入态才响应 Esc（回 Explore）、Delete（布局激活时改为删元素）、Home；剪贴板事件 → `ui.copy`/`ui.cut`/`edit.paste`；分派前有多种守卫（无选中不删、非 `{` 开头的粘贴丢弃等） |
| `command_search` | `chrome.rs:342` | 命令面板模态框：命中来源依次为地理处理工具（`gp:` 前缀）、命令注册表（跳过需参数的）、插件命令、内置常用 `ui.*` 条目；Enter 运行第一条 |
| `about` | `chrome.rs:480` | About 窗口（版本、简介、覆盖率 `catalog::coverage()`、许可）与快捷键速查窗口 |
| `notices` | `chrome.rs:544` | 通知 toast：普通 3 秒、错误 6 秒；右下角最多 3 条自下而上堆叠 |

---

## 3. `ribbon.rs` — 功能区

文件主旨：标签条（Project 按钮 + 固定/插件/上下文标签）与每个标签页内的组。设计原则：**每个控件都按命令 id 运行命令**，UI 不实现业务逻辑。

### 3.1 结构

- 固定标签 10 个（`TABS`）：Map、Insert、Analysis、View、Edit、Imagery、Share、Plugins、Settings、Help。
- 上下文标签（随选中对象出现）：要素层 → Feature Layer/Labeling/Data；栅格层 → Raster Layer/Raster Data；打开的布局 → Layout；插件可声明 `plugin:<id>` 自有标签。
- 组织三层：标签页 → 组（`group`，居中组标签+分隔线）→ 列（`col`，至多三个小控件）→ 控件（`big`/`small`/`big_menu`/`small_menu`）。
- `show`（`ribbon.rs:59`）：纠正失效标签 → 画标签条 → 最小化时直接返回 → 横向 ScrollArea 内按 `app.ui.tab` 分发到各 `*_tab` → 处理"窥视(peek)"。

### 3.2 固定标签页速览

| 标签 | 函数 | 主要组与绑定命令 |
|---|---|---|
| **Map** | `map_tab`（352） | Navigate（`tool.set` explore、`map.zoomFull/In/Out`、书签、Go To XY）、Layer（`map.basemap` 画廊、`ui.addData`）、Selection（`tool.set` select、`ui.dialog` selectByAttributes、`ui.gp.open` selectByLocation）、Inquiry（测距测面）、Offline、Feature Cache、Labeling（`labels.*`） |
| **Insert** | `insert_tab`（700） | Project（`map.new`、`layout.new`、`ui.openItem`）、Report / Presentation / Notebook（各自 `ui.new*` 与页添加）、Layer Templates（`layer.template`）、Styles、Graphics（各类 `tool.set` graphic*）、Data（addField/calculateField 对话框） |
| **Analysis** | `analysis_tab`（1097） | Geoprocessing（历史/工具箱/模型构建器/脚本/环境）、AI（`ui.pane.assistant`）、Portal（要素/栅格分析工具菜单）、Workflows、Raster Functions、Tools 画廊 11 个常用工具（Buffer/Clip/Erase/… → `ui.gp.open`）、Selection |
| **View** | `view_tab`（1285） | View（Globe、`map.projection`、Global/Local）、Effects（日照/天气/视觉/后期/色觉模拟 → `map.effects`）、Scene（线框、图层优先级、裁剪）、Windows（各面板开关）、Display（经纬网、暗色模式）、Navigation、Link（双视图/联动）、Workspace、Thumbnail、Animation、Profile |
| **Edit** | `edit_tab`（1752） | Manage Edits（`edit.enable/undo/redo`）、Snapping（`edit.snapping`）、Features（Create → `tool.set` create*、Delete/Modify）、Tools（merge/explode/flip/rotate/scale）、Elevation、Ground To Grid、Topology |
| **Imagery** | `imagery_tab`（2849） | 全部经 `ui.pane.workflows` 打开对应节：ortho/reality/georeference/change/spectral/classify/supervised/deepLearning/hosted/mensuration/process/indices/pixels |
| **Share** | `share_tab`（1902） | Output（`ui.exportMap/exportLayer/printMap/captureMap`）、Package（`ui.saveItem` 各类包）、Share As（`ui.share` webMap/webLayer/webStyle/locator/…）、Layout、Save As |
| **Plugins** | `plugins_tab`（2405） | Plugin Manager（`ui.plugins`）、Install From File；有命令/工具的插件各成一组；`plugin_tab` 渲染插件 manifest 声明的自有标签 |
| **Settings** | `settings_tab`（2519） | Language（`ui.language`）、Worldview（`ui.worldview`）、Start-up 三项（`ui.startup`） |
| **Help** | `help_tab`（2605） | Customize（Ribbon 显隐、Quick Access）、Diagnostic Monitor、Learning Resources、About/Command Search/Shortcuts |

### 3.3 上下文标签要点

- **Feature Layer**（`feature_layer_tab`，2677）：可见比例尺范围（`layer.scaleRange`）、透明度（`layer.transparency`）、符号系统（`layer.symbology` 单一/唯一值/分级/比例）、颜色/描边/大小、掩膜/显示过滤/聚合、3D 拉伸与光照、缩放到图层/属性表/移除。
- **Labeling**（3419）：标注开关（`labels.enable`）、比例尺/距离范围、放置位置、字段、字号/颜色/加粗/光晕（`labels.style`）。
- **Data**（3526）：定义查询、属性表、选择集操作（`select.all/switch/layerFromSelection`）、字段设计、归档（`data.archive`）、关系类、导出。
- **Raster Layer**（3177）：卷帘对比（`layer.swipe`）、混合模式、渲染器（拉伸/RGB/分类/唯一值/色带/晕渲 → `raster.symbology`）、拉伸类型/DRA/重采样/波段组合、亮度对比度伽马（`raster.enhance`）。
- **Layout**（2194，`layout_tab`）：页面尺寸、地图框（含手绘 polygon/lasso）、整饰要素（指北针/比例尺/图例 → `layout.addElement`）、文本/动态文本/图片、导出/打印。新增元素统一走 `layout.addElement` 闭包并自动回写选中。

### 3.4 重要辅助

- `tab_strip`（183）：标签宽度自适应、活动标签自动滚入视野、**双击标签折叠功能区**、右侧折叠按钮（Ctrl+F1）与当前工具指示器。
- `basemap_gallery`（606）：底图画廊，缩略图用代码绘制（无第三方图片资源）。
- `live_drag`（3053）：带实时预览的拖拽控件——拖动中直接写项目触发重绘，松手前先还原旧值再执行命令，保证撤销快照记录旧值。
- `GALLERY`（1083）：Analysis 标签 11 个常用工具的画廊数据。

---

## 4. `widgets.rs` — 功能区控件

文件主旨：大/小按钮、菜单按钮、组与列的封装；**命令禁用时控件自动置灰**。

| 控件 | 位置 | 说明 |
|---|---|---|
| `enabled` / `why_disabled` | 32/39 | 查命令注册表的可用性谓词与禁用原因（`ui.*` 与插件命令视为可用） |
| `tooltip` | 44 | 富提示：标题(+快捷键) + 描述 + 斜体"不可用: 原因" |
| `big` / `small` | 83/127 | 大按钮（32px 图标 + 一至两行标签）/ 小按钮（18px 图标）；点击执行命令 |
| `big_menu` / `small_menu` | 156/165 | 主按钮 + 弹出菜单（分割按钮式） |
| `menu_item` | 185 | 菜单行：图标 + 标签 + 快捷键；禁用即整行禁用 |
| `group` / `col` | 224/255 | 功能区组（控件横排 + 居中组名 + 竖分隔线）/ 竖排小列 |
| `ribbon_combo` | 264 | 功能区紧凑下拉 |
| `primary_button` / `dialog_button` | 283/306 | 手绘居中的实心主按钮 / 对话框按钮 |
| `centred_label` | 325 | 按大写字母中线（而非字体行盒）居中，规避字体 ascent/descent 不均 |
| `icon_button` | 341 | 26px 自绘图标按钮（字体可能缺字，故不依赖字形） |

---

## 5. `theme.rs` — 设计令牌、字体与样式

| 项 | 位置 | 说明 |
|---|---|---|
| `BRAND` / `CONTEXT` | 8/11 | 品牌色 `#B84A2C`；上下文标签组色 `#0E9384` |
| `Tokens` | 14 | 亮/暗各约 30 个颜色字段（结构面/边框/文字/交互态/强调/语义色），原则是**控件绝不硬编码颜色**；`light()`/`dark()` 构造，`get()` 按 egui `dark_mode` 现取 |
| `install_fonts` | 137 | 注册系统无衬线字体为 `"ui"`/`"ui-semibold"` 两族；`extra` 回退字面（CJK）按当前区域决定插在第 1 位（字形优先）还是末尾；`ctx.set_fonts` 生效（当帧不生效，故 `lib.rs` 前两帧跳过绘制） |
| `apply` | 171 | 把 Tokens 映射到 egui `Visuals`（面板填充、圆角 10/8、控件三态、阴影）+ spacing + 文本样式（Body 12.5、Heading 半粗 17 等） |
| `c32` / `rgba` | 219/223 | 引擎 `Rgba` ↔ egui `Color32` 转换 |

---

## 6. `icons.rs` — 图标

文件主旨：**全部图标为原创、在 24 单位网格上用代码绘制**（中性墨线描边 + 强调色填充），不引入任何第三方图标库/字体字形，与项目"无第三方资产"约定一致。

- 绘图上下文 `P`（5-155）：坐标换算（`s`/`pt`/`w`）+ 基元（`line`/`closed`/`fill`/`rect`/`circle`/`arc`/`dashed_rect`/`text`/`tint`）+ 复合部件（`globe`/`magnifier`/`cursor`/`page`/`table`/`plus_badge`/`layers`/`polygon_shape`）。
- `paint`（158）：唯一入口，按名称巨型 `match`；**未知名称回退为圆角描边方块，永不 panic**。
- `ALL`（931）：约 120 个图标名常量。族：浏览导航、数据工程、选择、测量识别、标注、地理处理、表与字段、输出分享、符号化、栅格、绘制草图、整饰要素、UI 杂项；`logo`（638）为齿轮 + 四色地球的品牌标。

---

## 7. `panes.rs` — 停靠面板（Contents/Catalog/Geoprocessing/Symbology/History/Details）

| 面板 | 函数 | 说明 |
|---|---|---|
| 框架 | `show`（17）/`pane_header`（87） | 左面板（Contents/Catalog，宽 200-480）与右面板（18 种，宽 260-560；Assistant 不经滚动区）；每面板 34px 标题栏 + 关闭按钮 |
| **Contents** | `contents`（156）/`layer_row`（225） | 绘制顺序列表：展开箭头、可见性复选（`layer.visibility`）、符号色块（`patch`，107）、图层名（活动加粗/filtered/置灰）、选中徽章；单击选中、双击开符号面板；右键菜单（缩放到图层/属性表/标注/上移下移/重命名/导出/删除）；展开显示图例；空状态给 Add Data 与示例数据按钮 |
| **Catalog** | `catalog`（445）/`folder_browser`（624） | 可折叠分组：Maps、Toolboxes（内置工具树 → `ui.gp.open`）、Layouts、Styles、Tasks、Folder Connections（非 wasm）、Tables、Folders（本地目录浏览，双击按扩展名分派打开命令） |
| **Geoprocessing** | `GpForm`（707）/`geoprocessing`（953） | 两形态：①工具查找器（搜索 + 插件/GeoLibre/内置三源浏览）；②工具表单（`GpForm::open` 按参数类型预填图层，`ParamKind` 生成控件，Run 组装 JSON → 同步 `gp.run` 或异步 `external_start`）。`poll_geolibre`（737）每帧轮询浏览器 GeoLibre 的清单/运行结果槽；`geolibre_card`（816）桌面一键安装卡片（后台线程 + 进度条） |
| **Symbology** | `symbology`（1311）/`raster_symbology`（1721） | 矢量：主符号类型卡片 → 按渲染器分支（字段/色带/分级方法/类数/`symbol_editor`）；栅格：渲染器卡片（按数据条件禁用）→ 波段/色带/拉伸/DRA/重采样/增强；辅助 `ramp_picker`/`legend_list`/`field_combo` |
| **History** | `history`（1536） | 倒序处理历史卡片（成功/失败圆点、耗时、消息），Clear → `gp.clearHistory` |
| **Details** | `details`（1567）/`raster_details`（1665） | 活动图层摘要：矢量（要素数/字段/数据源/四至/定义查询 + 字段清单）；栅格（尺寸/波段/像元类型/格网/坐标系/NoData/各波段统计表） |

---

## 8. `mapview.rs` — 地图视图

文件主旨：把渲染后的地图显示为纹理（变化时重渲染；平移缩放中对上一帧做位移缩放以保证流畅），并承载交互工具、草图、标注、识别弹窗、比例尺、指北针与视图标签页。

### 8.1 核心结构

- `Key`（15）：纹理缓存键 = 会话修订 + 相机（量化）+ 像素尺寸 + 深色 + 草稿分辨率标志 + 瓦片代数 + 流式数据代数；任一变化触发重渲染。
- `MapView`（57）：纹理与缓存、已放置标注、当前拖拽（`DragKind`: Pan/Box/Sketch）、草图顶点、拆分模式、放大镜、识别弹窗（`Popup`，50）、指针经纬度/DMS、渲染耗时、Toast 等。

### 8.2 渲染流程

`show`（143）→ 视图标签页 → 有活动布局转 `layoutview::show`；目录视图转 `catalog_view`；否则 `animate`/`split_view`/`main_view`。

`main_view`（250）核心：

1. 分配交互区、处理一次性起始范围、`interact` 处理输入；
2. 重渲染判定：静止时全清渲染；地球运动中以 0.6×ppp 降清草稿渲染，静止后再全清；
3. 调引擎 `render::render(map, &view)` 得 pixmap → egui 纹理；保存标注/瓦片署名/渲染耗时；
4. 绘制：先铺底色，再按旧→新相机映射把旧帧画在过渡矩形上实现平移缩放动画；
5. 标注随纹理位移（缩放/地球运动中隐藏防闪）；最后 `overlays` 与 `popup`。

### 8.3 交互（`interact`，454）

- 滚轮以指针为锚缩放；第一人称 WASD/QE 行走（`map.walk`）；方向键平移。
- 拖拽：中键或 Explore → Pan；Shift+Explore → 框缩放；Select/ZoomIn → 框选/框放大；图形工具 → Sketch（freehand/lasso 逐点采样上限 20000）。
- 单击按工具分派：Explore → `identify`（命中弹 `Popup` 并选中）；Select → `select.atPoint`；CreatePoint → `edit.addFeature`（带模板）；测量/逐顶点工具 → 压草图顶点；拆分模式 → `edit.split`。
- 双击：测量/创建线面 → 收尾；否则 2× 放大。
- 右键菜单：缩放/全图/复制坐标/清除选择。

### 8.4 覆盖层（`overlays`，833）

框选高亮、编辑顶点、导航罗盘（`navigator`，1272）、第一人称准星、放大镜（70px 圆形镜头）、工作流取点标记、吸附预览、拖拽图形轮廓、草图绘制（测地长度/面积胶囊提示）、工具提示条、比例尺（`scale_bar`，1095）、指北针（`north_arrow`，1115）、瓦片署名、右上角缩放按钮。

### 8.5 识别弹窗与标签页

- `popup`（1128）：锚点圆环、多结果翻页、按 name 字段取标题、斑马纹属性表、可拖拽调整大小。
- `view_tabs`（1305）：每地图一个标签（→ `map.activate`）+ 布局标签 + "＋"（`map.new`）。

---

## 9. `layoutview.rs` — 布局设计视图

文件主旨：打开布局时替代地图显示页面与元素；点击选中、拖动移动、拖控制柄缩放、Shift 拖地图帧平移内部地图、滚轮缩放页面；右侧属性面板编辑选中项。

| 项 | 位置 | 说明 |
|---|---|---|
| `Drag` | 12 | 拖动模式：Move / Resize（8 柄）/ PanFrame / Pan / Lasso |
| `LayoutView` | 26 | 渲染纹理缓存键（修订/布局/DPI）、页面 zoom/pan、选中、绘帧工具与草点、属性输入草稿 `typing`（按 (布局,元素,字段) 键控防丢按键）、跨帧共享 `Rc<Layout>`（避免克隆含大图的布局） |
| `show` | 105 | 主入口：属性面板（270px）→ 背景 → 交互 → 按屏幕分辨率重渲染页面纹理（DPI 钳 20-300）→ 阴影/白页/纹理/选中框/控制柄 → 底部状态条（mm 尺寸、缩放%、提示）与系列页标签 |
| `interact` | 253 | 滚轮缩放（0.2-12×）；polygon 点击加点双击完成、lasso 拖拽采样；单击选中；双击地图帧 `layout.frameZoom`；拖拽起手判定柄/元素/空白；松手位移 >0.2mm 才 `layout.update`（可撤销） |
| `properties` | 423 | 选中元素属性：名称/X/Y/宽高(mm)/可见；按类型追加（Text 字号颜色、Legend 标题、MapFrame 地图下拉与缩放按钮、Table 行数）；底部 Delete → `layout.removeElement` |

---

## 10. `table.rs` — 属性表

文件主旨：停靠地图下方的属性表——排序、过滤到选中集、行选择（与地图联动）、就地编辑。

- 状态 `TableState`（13）：独立表名、排序列、仅选中、编辑缓冲、文本过滤。
- `show`（23）：底部面板（默认 250px，120-600 可拖）；数据源优先 `session.table_layer`（可编辑、有选中集），否则独立表。
- 表头工具：按属性选择、缩放到选中、反转/清除选择、删除选中行、添加/计算字段、"仅选中"复选、行过滤框。
- 行集：文本过滤为任意属性包含匹配（忽略大小写）；排序比较失败按 Equal 处理（避免 panic）。
- 单元格：Null 斜体置灰；**双击行内编辑**（`edit.setValue`）；**单击选行**（Cmd/Ctrl/Shift 加减选 → `select.rows`），与地图选择同步。

---

## 11. `dialogs.rs` — 对话框与原生文件选择器

- 状态 `Dialogs`（11）：当前对话框名、目标图层、文本与三个通用输入位、模式、错误/信息。
- `Dialogs::open`（26）：白名单校验 12 个对话框名 + 前置条件检查（需选中图层等）+ 预填默认值。
- 12 个对话框：`selectByAttributes`（SQL 实时校验 + 字段 chips）、`goToXY`、`bookmark`、`addField`、`calculateField`、`definitionQuery`、`renameLayer`、`coordinateSystem`（双栏：预设/EPSG + 详情含 PROJ 定义与适用范围）、`graphicText`、`camera`、`addTiles`（XYZ/TMS/WMS/PMTiles/MapLibre，`TileKind::guess` 自动判型）、`exportLayer`（9 种格式，Web 隐藏 GeoPackage、shp 打包 zip）。
- `pick_open`/`pick_save`（460）：桌面 `rfd::FileDialog`；**wasm 恒返回 None**（Web 走 `Services::open_async`）。

---

## 12. `backstage.rs` — Project 后台页

文件主旨：新建（空白/样例）、打开、保存、另存、关于。

- 左导航（210px，品牌色底）6 页：Start/New/Open/Save/Save As/About；顶部返回按钮。
- Start 页（88）：欢迎区 + 三张卡片（空白地图 `project.new`、World Atlas `project.sample`、打开 `ui.open`）+ 当前工程信息网格。
- About 页（151）：版本、简介、官网/GitHub 链接、覆盖率、许可声明。

---

## 13. `analysispane.rs` — Workflows 与 Script 面板

### 13.1 Workflows

- `Workflow` 状态（19）：当前节、图层/字段/数值/文本参数、栅格函数、地图拾取 `Pick`（11：用途/需求数/已采点）、结果 JSON、错误。
- `SECTIONS`（71）：28 个节——rasterFunctions、functionEditor、suitability、visibility、lineOfSight、interpolate、network、neighborhood、simulation、profile、explore、similar、interop、environments、models、georeference、change、spectral、classify、supervised、deepLearning、hosted、indices、process、pixels、mensuration、ortho、reality。
- `workflows`（116）：节选择下拉（切换清结果）→ 按节自动回填默认图层 → 28 分支表单 → 尾部拾取进度/错误/结果。
- 地图取点：`pick_button`（103）启动/取消，`picked`（111）达标取走；地图点击由 mapview 写入 `pick.points`。
- 运行是**同步**的（`run`，52）：无后台进度；量测结果直接读 `session.measurements`、编辑状态读 `session.edit_counts`（避免每帧跑命令抬高修订号）。
- 结果渲染 `result_view`（708）：字段剖面表、直方图、其余格式化 JSON 截 3000 字符等宽显示。

### 13.2 Script

`script`（754）：一行一条 `命令id {json}`、`#` 注释；Run → `script.run`，逐行输出结果（错误红色）；"Commands…" 打开命令搜索。

---

## 14. `assistantpane.rs` — AI 助手面板

文件主旨：与用户自选模型的对话，模型通过**运行命令**作用于地图；请求在帧外发出（桌面线程，Web 经 `Services::http`）。

- `AssistantState`（22）：agent、输入框、排队消息 `queued`、在途请求 `inflight`（代次 + Slot）、设置与密钥管理、模型列表、输入历史、审批"本会话允许"标志。
- `dispatch`（114）：先 `provider::allowed_url` 校验（仅 HTTPS 或本机/内网 HTTP），优先 `Services::http`，桌面 `thread::spawn`。
- `poll`（181）每帧：保存无焦点的设置改动 → 取 `queued` 消息发送 → `inflight` 有结果则 `agent.receive(代次,…)`（代次丢弃过期响应）→ `follow` 执行下一步（命令或再请求）→ 模型列表槽更新。
- `show`（236）：标题栏（提供方·模型，可点开设置）→ 可折叠设置区 → 贴底滚动会话记录 → 底部输入区。
- 消息流 `transcript`（480）：User 右对齐主题色；Assistant 走迷你 Markdown 渲染（`markdown`，543：代码块/标题/列表/引用/表格/行内粗体斜体代码链接）；Tool 条目等宽折叠（✓/✗）；Error 红色；等待中转圈"Thinking…"。
- **审批流程** `composer`（391）：`agent.pending` 取待审批动作 → 琥珀色卡片（动作内容 + 原因）→ Allow/Decline → `agent.decide` → `follow`；`can_always` 提供"本会话允许"复选，**插件命令/脚本/替换项目强制每次询问**；新提问重置 `always`。
- 快捷键：Enter 发送、Shift+Enter 换行、↑/↓ 翻历史；忙时显示 Stop。
- 安全细节：`with_agent`（167）临时借出 agent 执行闭包，期间新增的**错误类通知被过滤**，避免命令错误既进会话记录又弹通知。

---

## 15. `editpanes.rs` — Editing 与 Data Design 面板

### 15.1 Editing（`editing`，100）

- 总开关：`edit.enable`、显示顶点 `edit.settings`。
- 修改选中要素：移动（东/北米）、旋转、缩放 → `edit.move/rotate/scale`（数字经 `number()` 校验，非法写错误消息而非当 0）。
- 快捷工具：Merge/Explode/Flip Line、Split（切换 `map.split_mode` 后在地图上点线）。
- 要素模板：选中/删除模板（`edit.template`）、"From Selection" 用首条选中要素建模板。
- 捕捉：Vertex/Edge/End + 容差（`edit.snapping`）。
- 高程：None/Constant/Surface（`edit.elevation`）。
- 地面到格网：比例 + 旋转（`edit.groundToGrid`）。
- 拓扑：规则增删（mustNotOverlap 等 5 类 → `topology.rules`）、Validate（`topology.validate`）、错误列表点击可缩放并选中要素。
- 编辑状态：直接读 `session.edit_counts` 显示各图层增/改/删计数。

### 15.2 Data Design（`design`，337）

需先选中要素图层。按命令分块：

- 值域 Domains：Coded（`code=描述` 多行）/ Range（min-max）→ `data.domain`。
- 子类型 Subtypes：整数字段 + `code=name` 多行 → `data.subtypes`。
- 属性规则：Calculation（字段+表达式）/ Constraint（恒真 SQL）→ `data.attributeRule`。
- 相依值：多行有效组合 + 名称 + 字段列表 → `data.contingentValues`。
- 归档：开关（`data.archive`）+ 按"N 分钟前"回溯（`data.archiveRestore`）。
- 关系类：源/目标图层+键字段+关系名 → `data.relate`；已有关系可"Select Related"（`data.related`）或删除。

---

## 16. `pluginsui.rs` — 插件管理器

- 流程：浏览/搜索注册表 → Install/Update → `fetch` 帧外下载 → `poll` 收包 → `received` 三重校验（sha256、插件 id、黑名单）→ `confirm`（751）权限确认模态（"This plugin can:" 权限清单 + 沙箱说明）→ `install_confirmed` 落盘（桌面插件目录 / Web 偏好）→ Installed 标签可启用/禁用/卸载/更新。
- `show`（798）：窗口含 Installed/Browse/Settings 三标签；Browse 自虚拟滚动。
- `poll`：每帧检查下载槽；`restore`：Web 启动时恢复已装插件。

---

## 17. `docpanes.rs` — Presentation / Notebook / Shares 面板

| 面板 | 函数 | 说明 |
|---|---|---|
| Presentation | `presentation`（60） | 页列表与预览（`preview`，252：`catch_unwind` 包裹 30dpi 渲染）、放映定时翻页、字段编辑失焦保存（`presentation.updatePage`）、增删移页（mapPage/blankPage/image/video）、导出 |
| Notebook | `notebook`（282） | 单元格卡片（Commands/Text）；Code 格 Run 前先落盘编辑草稿；编辑与底层不一致时丢弃草稿；失焦差异 → `notebook.editCell`；输出等宽、错误红色；Run All、增删单元格 |
| Shares | `shares`（375） | 共享项列表（类型·路径）与管理按钮（Save Web Map/Scene/Link Chart、Replace Web Layer）；Data Stores 区可移除数据存储，桌面端可添加存储与上传文件 |

---

## 18. `workspacepanes.rs` — 工作区面板

| 面板 | 函数 | 说明 |
|---|---|---|
| Tasks | `tasks`（59） | 任务下拉 + 步骤列表（当前步高亮）；每步 Run → `task.run`，成功步进并提示；全部完成显示 All steps done 与 Start Over |
| Jobs | `jobs`（117） | Workflow Manager：新建作业（`job.new`，覆盖当前视图范围）；按 new/in progress/on hold/done 分组；每条可 Zoom、状态流转（`job.update`）、删除（`job.remove`） |
| Profile | `profile`（174） | 高程剖面：选栅格 + 三种拾取模式（Create 100 点 / Auto-complete 2 点 / Cross Section 2 点）；沿选中线（`profile.alongLine/alongSelected`）；结果手绘折线图 + 长度/升降统计 |
| Diagnostics | `diagnostics`（295） | 诊断监视器：500ms 刷新实时帧率与地图绘制耗时；每秒调 `workspace::diagnostics` 补充其余指标 |
| Infographics | `infographics`（321） | 信息图卡片（按视图键缓存）：每图层视图内要素数大字、数值字段合计、Top 分类条形图 |
| Catalog View | `catalog_view`（366） | 替代地图的目录视图：地图/布局/表/样式/任务/模型/文件夹/离线区域卡片网格，双击执行对应激活命令后返回地图 |

---

## 19. `reportpane.rs` — Report 面板

- 状态 `ReportState`（12）：页码、预览缓存、已构建报告（按 `(报告索引, 修订)` 缓存，翻页只重画不重建）、统计字段下拉、编辑草稿。
- `report`（50）主面板：无报告则提示新建；报告区（Title/Where SQL/仅选中）；Fields 区（逐字段复选、Autofill、排序）；Groups 区；Statistics 区（count/sum/mean/min/max）；图片/附件/图表字段下拉；Sections 区（补充页/布局页/关联报表/子报表）；底部 Export/Save As。
- `preview`（279）：`catch_unwind` 包裹构建与绘制（失败也缓存避免每帧重试），60dpi 渲染、纹理宽 ≤300px、◀/▶ 翻页。

---

## 20. `i18n/mod.rs` — 国际化

文件主旨：代码中的英文字符串作为**查找键**，每种语言一份 `xx.tsv` 目录在绘制时映射为显示文本。**命令 id、项目文件、CLI、数据永不翻译**；无翻译回落英文，覆盖率可逐条增长。

| 项 | 位置 | 说明 |
|---|---|---|
| `AUTO` | 25 | 偏好值 `"auto"`：跟随系统语言（`RUSTGIS_LANGUAGE` → OS/浏览器） |
| `LangInfo` / `lang!` | 43/55 | 语言条目：code、自身名、英文名、文字系统、编译期 `include_str!` 内嵌目录、惰性解析缓存 |
| `LANGUAGES` | 62 | 15 种语言（de/es/fr/it/nl/pt/ru/tr/az/id/vi/zh/ja/ko，英语居首作回退）；繁体中文回落英语而非简体文本 |
| `parse` / `LangInfo::catalog` | 103/122 | 目录解析：`英文<TAB>翻译`，`#` 注释，`\n`/`\t` 转义，`{name}` 占位符必须保留 |
| `set_current` / `t` / `tr` | 222/236/242 | 线程局部当前语言；`tr` 命中目录 → 英语原文 → 插件翻译 `plugin_tr`（带修订+语言键控的帧级缓存）→ 回落原文 |
| `location` | 294 | 位置串（如 `Map › Navigate`）按 ` › ` 分段逐段翻译 |
| `fmt` | 300 | 先翻译模板再**单遍**填充占位符——值不再被二次解析（名为 `{n}` 的图层不会被替换） |
| `tl!` 宏 | 326 | `tl!("Explore")` → 当前语言取词 |

使用约定：界面可见字面量一律 `tl!("…")`；带变量的消息用 `i18n::fmt("… {name} …", &[("name", …)])`；新增字符串须同步补进每个 `i18n/*.tsv` 目录（格式见 `docs/i18n.md`）。

---

## 附：模块协作总览

```
                    ┌─────────────── lib.rs (RustGisApp) ───────────────┐
                    │  logic(): 轮询 Slot/Inbox、字体、主题               │
                    │  ui():    header → ribbon → panes → table → map   │
                    │  run():   ui.* → run_ui | 其余 → session.run       │
                    └──┬──────────┬──────────┬──────────┬───────────────┘
                       │          │          │          │
     chrome.rs     ribbon.rs   panes.rs   mapview.rs  dialogs.rs
   标题栏/状态栏   10+上下文    左右面板     画布渲染     12 个对话框
   快捷键/命令搜索  标签页分发   18 种右面板   交互/草图    rfd 选择器
                       │          │
                   widgets.rs  ┌──┴──────────────────────────────┐
                   icons.rs    │ analysispane (Workflows/Script)  │
                   theme.rs    │ assistantpane (AI, HTTP 轮询)    │
                   i18n        │ editpanes (Editing/Design)       │
                               │ pluginsui (插件管理)             │
                               │ docpanes (演示/笔记本/共享)      │
                               │ workspacepanes (任务/作业/剖面)  │
                               │ reportpane (报表)                │
                               │ layoutview (布局设计)            │
                               │ backstage (Project 后台页)       │
                               └──────────────────────────────────┘
```



# 开发实录：「移除所有图层」按钮

> 2026-10-14。目标：在虾神格式转换插件的选项卡上再加一个按钮，点击后把当前地图工程
> 内容列表里**所有已经加进来的图层全部移除**。
>
> 结论先行：这次宿主一行都不用改——`layer.remove` 已经认 id、权限 `edit` 插件早就声明了、
> "插件命令 = 一个撤销步骤、失败整体回滚"是宿主白送的。真正要拿捏的是三件事：
> **先取快照再删**、**中途失败必须整体回滚**、**破坏性操作没有确认框怎么办**。
> 本文记录全过程与验证方法，供后人做同类"批量改动工程"的插件命令参考。

---

## 1. 需求拆解

| 需求 | 落点 |
|---|---|
| 选项卡上多一个按钮 | `plugin.json`：新命令 `clear-layers` + 挂进「图层」分组 |
| 点击即执行（不要弹窗填参数） | 命令**不带 `form`** → 宿主直接运行，不打开对话框 |
| 把内容列表里所有图层都移除 | `host.layers` 拿列表 → 逐个 `layer.remove` |
| 别把工程搞坏 | 失败整体回滚 + 一步撤销 + 状态栏报告数量 |

关键认知（对齐 `docs/writing-a-plugin.md` 的选型表）：
**"对图层做改动、还要撤销/回滚"是命令 + `edit` 权限的典型场景**，不需要新工具（tool）、
不需要新权限、不需要补丁。这次的按钮是三个按钮里（格式转换、图层信息、移除所有图层）
宿主侧成本最低的一个。

---

## 2. 动手前的调研：宿主已经给了什么

按 `DEVELOPMENT.md` §9 的 checklist，先查 `docs/plugins.md` 的请求表和
`crates/engine/src/cmd/plugins.rs` 的权限检查，再决定代码怎么写。四个关键事实：

**① `layer.remove` 认 id 也认名字**（`crates/engine/src/cmd/layer.rs`）：

```rust
C::new("layer.remove", "Remove", "Contents", remove).key("Delete").when(need_layer)
     .params(r#"{"layer"?: name|id}"#),
```

`Session::layer_id`（`crates/engine/src/lib.rs`）里 `Value::Number` 走 id、`Value::String`
先按名字找再退回解析成 id，所以 `{"layer": 1}` 和 `{"layer": "Cities"}` 都行。

**② `host.layers` 每层都给 `id`**（`Bridge::layers`，`crates/engine/src/cmd/plugins.rs`）：
`{"id": l.id, "name", "raster", "visible", "path", "geometry", "features", "selected",
"active", "fields"}` —— id 是稳定的数字，删除过程中不会重排（重排的是列表位置，不是 id）。

**③ `when(need_layer)` 不会挡住逐个删**。这是最容易踩空的一条：`layer.remove` 带
`when(need_layer)`，表面看"必须先选中图层"。但桥接层 `Bridge::command` 收到显式
`layer` 参数时会**临时把活动图层指向它**再跑 `enabled` 检查：

```rust
let named = params.get("layer").and_then(|_| self.s.layer_id(params).ok());
let active = self.s.active_layer;
if named.is_some() { self.s.active_layer = named; }
let blocked = catch_unwind(... enabled(self.s) ...);
self.s.active_layer = active;
```

所以只要每次 `remove` 都显式带 id，就不依赖界面上的选中状态。

**④ 权限与撤销是宿主白送的**。`refused()` 按清单 `permissions` 校验，本插件为
"格式转换"早就声明了 `edit`，不用新增；`Session::run_plugin` 把整个插件命令包成
**一个 undo 步骤，命令失败整体回滚** —— 逐个删中途出错不会留下删了一半的工程。

> 调研的副产品：一开始担心"要不要自己写确认框/事务"，查完发现**不需要**——
> 沙箱本来就不让插件弹确认框（见 `DEV-JOURNAL-host-dialog-and-layer-info.md` §1），
> 而宿主的单步 undo 已经是事务。设计因此大幅简化。

---

## 3. 设计取舍

### 3.1 先取快照，再逐个删

删除过程中宿主的 `layers` 列表会变（索引前移、活动图层漂移），**边查边删**会漏。做法是
一次性把 `host.layers` 的结果收成 `targets`，再对着这份快照逐个 `layer.remove`：
位置会变，**id 不变**，所以按 id 删是安全的。

### 3.2 任何一步失败 → 直接冒泡，不做"跳过继续"

```rust
for target in &targets {
    host_call("layer.remove", &json!({"layer": target}))?;   // ← Err 直接返回
    removed += 1;
}
```

"能删几个删几个"看似更稳健，实则把工程留在**半删状态**且无法解释删了哪些。
宿主的语义是失败整体回滚——返回 `Err` 等于免费拿到事务性，跳过反而破坏它。
同理，列表里出现既没 `id` 也没 `name` 的图层时也是**报错而不是跳过**：

```rust
if targets.iter().any(Value::is_null) {
    return Err(tr(lang, "有的图层没有 id 或名称，无法移除", "…"));
}
```

### 3.3 破坏性操作不加确认框

沙箱不让插件弹窗，宿主的 `form` 又只有 `path`/`text`/`number` 三种输入（没有"确认"语义）。
权衡后**不加确认**，改用三层兜底：状态栏明确报告"已从当前工程移除全部 N 个图层"、
**一步 `Ctrl+Z` 全部恢复**、命令本身不会崩。这是取舍不是疏漏——若日后需要强确认，
正路是给宿主的 `form` 加 `confirm` 字段（走补丁），而不是让插件绕沙箱。

### 3.4 空列表不碰宿主命令

内容列表为空时只发一次 `host.layers`，直接 `Err`（中/英随 `lang`）。
既避免无意义的 notify，也让"点了没反应"变成一句可读的话。

---

## 4. 实施步骤

改动全部在 `plugins/xiashen-format-converter/` 内，**源工程一个文件没动**。

### 4.1 `plugin.json`

```json
{
  "id": "clear-layers",
  "label": "移除所有图层",
  "help": "把内容列表里的所有图层从当前地图工程中移除（整个命令是一步撤销）。",
  "icon": "delete",
  "params": "{}"
}
```

- 无 `form` → 点击即运行（有 `form` 的命令才会打开对话框）；
- `icon: "delete"` 先在 `crates/ui-egui/src/icons.rs` 里确认存在（`"delete" => {…}`，
  且在内置图标名列表里），未知名会显示拼图占位；
- `tab.groups` 的「图层」组从 `["layer-info"]` 变成 `["layer-info", "clear-layers"]`。

### 4.2 `src/lib.rs`

在 `mod wasm` 里加 `clear_layers(lang)`，并在 `command()` 的分发表里挂上
`"clear-layers" => clear_layers(lang)`（插件是手写 ABI 胶水，没有 SDK 的声明宏，
**分发表就是命令注册表的插件侧一半**，漏挂就报"未知命令"）：

```rust
fn clear_layers(lang: &str) -> Result<Value, String> {
    let layers = host_call("host.layers", &json!({}))?;          // 1) 快照
    let list = layers.as_array().map(Vec::as_slice).unwrap_or(&[]);
    if list.is_empty() {
        return Err(tr(lang, "内容列表里还没有图层", "the content list has no layers yet"));
    }
    let targets: Vec<Value> = list.iter().map(|l| match l.get("id") {
        Some(id) if !id.is_null() => id.clone(),                 // 2) id 优先，名字兜底
        _ => l.get("name").cloned().unwrap_or(Value::Null),
    }).collect();
    if targets.iter().any(Value::is_null) { … return Err(…) }    // 3) 缺标识 → 报错

    let mut removed = 0usize;
    for target in &targets {                                     // 4) 逐个删，失败冒泡
        log_line(&format!("移除图层 {target}"));
        host_call("layer.remove", &json!({"layer": target}))?;
        removed += 1;
    }
    host_call("host.notify", &json!({"text": tr(lang, …)}))?;    // 5) 报告数量
    Ok(json!({"removed": removed}))
}
```

顺手在文件头注释里补了一行"同一选项卡上还有图层信息与移除所有图层"，让目录级读者
不用翻 `plugin.json` 就知道插件有几个按钮。

### 4.3 `tests/e2e.rs`

新增 `removes_every_layer_end_to_end`；`Mock` 加一个 `empty: bool` 开关让 `host.layers`
返回 `[]`，用 `Mock { empty: true, ..Mock::default() }` 构造，**既有用例一个字不改**：

```rust
let mut host = Mock { empty: true, ..Mock::default() };
let e = rustgis_plugins::run_command(&cmd, &mut host, &Value::Null).expect_err("没有图层应报错");
```

断言四件事：请求序列、每次 remove 的参数、通知含个数、空列表只发 1 个请求。

### 4.4 文档

- `README.md`：需求表补第⑥行 + 新增一节「移除所有图层：快照后逐个删」；
- `DEVELOPMENT.md`：新增 §11（宿主侧为何不用改、测试怎么补）；
- 本文档：过程与经验的完整实录。

---

## 5. 验证（全部真实跑过）

| 层 | 命令 | 结果 |
|---|---|---|
| 构建 | `.\build.ps1` | wasm 编译通过，打包 36.4 KB |
| 插件 e2e | `cargo test --manifest-path .\Cargo.toml` | **4 passed**（含新用例） |
| CLI 真跑·正常 | `run --plugin … --sample --cmd clear-layers` | `Removed all 4 layers from the project`，返回 `{"removed": 4}` |
| CLI 真跑·空工程 | `run --plugin … --cmd clear-layers`（不带 `--sample`） | `移除所有图层: the content list has no layers yet`，退出码 1，不崩 |
| CLI 真跑·撤销 | clear → `edit.undo` → clear | 两次都 `{"removed": 4}` → undo 把 4 个图层全恢复了 |

**撤销验证是这次最值钱的一条**：不用启动 GUI，三条 `--cmd` 串起来就能证明
"整个命令是一个撤销步骤"——如果 undo 无效，第二次 clear 会撞上空列表报错退出码 1。

注意 CLI 输出里的通知是**英文**（`Removed all 4 layers…`）：`lang` 由宿主按界面语言给，
CLI 环境是 `en`，中文机上开桌面端会走 `tr()` 的中文分支，这是预期行为不是 bug。

---

## 6. 踩过的坑

### 6.1 PowerShell 单引号里的 JSON 路径坏了

第一版验证命令写成 `--cmd 'share.exportMap={"path":"$TEMP\cleared.png"}'`，报
`bad JSON: key must be a string at line 1 column 2`。两个错叠在一起：单引号里 `$TEMP`
不展开，而 `\c` 不是合法 JSON 转义（JSON 里反斜杠要写 `\\`）。**教训**：CLI 的 `--cmd`
参数里写路径，要么用正斜杠，要么先自己展开成绝对路径——JSON 的反斜杠是转义符不是分隔符。
（这条与插件无关，但会让验证第一眼像"功能坏了"。）

### 6.2 `build.ps1` 打包输出中文乱码

`已打?? E:\…rgplugin (36.4 KB)`——控制台代码页问题，**功能正常**（后面测试与 CLI 加载
都证明包是好的）。判断构建成败看 cargo 的 `Finished` 和文件是否生成，别看乱码。

### 6.3 git status 里一堆不是我改的文件

源工程有大量用户在做的 `form` 衹丁与 `host.dialog` 合入（`crates/**`、`docs/**`、
14 个 i18n `.tsv`）。**本次所有编辑都用工具逐个定点改，且只落在 `plugins/` 下**，
对照 `git status --short` 确认没把别人的半成品混进来。踩坑点：插件目录是**整目录
untracked**（`?? plugins/`），看不出自己改了里面的哪几个文件——自查只能靠"记得改了什么"
+ 逐文件复读。

### 6.4 空列表分支的断言要语言无关

错误消息中/英随 `lang`，e2e 里写死中文或写死英文都会在另一种环境下挂：

```rust
assert!(e.contains("图层") || e.contains("layers"), "{e}");   // 两种都算过
```

与上一份实录 §4.2 的 `i18n::t("Close")` 是同一个教训的插件版：**断言行为，不断言文案**。

---

## 7. 经验教训（可复用的）

1. **破坏性命令先问宿主"给了什么事务语义"**。插件命令 = 单个 undo 步骤 + 失败整体
   回滚，是写批量改动命令的地基；确认框做不了就别硬做，把力气花在"可撤销 + 报告清楚"。
2. **边遍历边修改，先取快照**。快照里的"位置"会失效，但**稳定 id 不会**——批量操作
   永远按 id，不按索引、不按名字（名字可被 `layer.rename` 改掉）。
3. **失败就整体失败，别"跳过继续"**。宿主会回滚，跳过只是把半成品状态甩给用户；
   事务边界内没有"部分成功"这种值。
4. **`when(条件)` 不等于"用户必须手动满足"**。桥接层会按参数临时改活动图层——
   看到 `.when(need_layer)` 先查 `Bridge::command` 怎么准备状态，再决定要不要额外传参。
5. **图标先去 `icons.rs` 查**，别凭感觉命名；拼图占位不好看也不好查。
6. **验证设计要动脑**：撤销这种"只有 GUI 才好点"的行为，用 `--cmd A --cmd B --cmd A`
   的差分就能在 CLI 里证伪，成本接近零。
7. **CLI 报错第一眼可能是你自己的参数**。功能没坏之前，先确认验证命令本身是合法 JSON。
8. **untracked 目录里自查改动**：插件目录整体 untracked，改完要么逐文件复读，
   要么在 commit 前 `git status` 单独盯这个目录，别指望 diff 提醒你。

---

## 附：改动文件清单

**插件（全部改动）**

- `plugins/xiashen-format-converter/plugin.json`：`clear-layers` 命令 + 「图层」分组加一项
- `plugins/xiashen-format-converter/src/lib.rs`：`clear_layers()` + 分发表挂载 + 头注释
- `plugins/xiashen-format-converter/tests/e2e.rs`：`Mock.empty` 开关 + `removes_every_layer_end_to_end`
- `plugins/xiashen-format-converter/README.md`：需求表第⑥行 + 新章节
- `plugins/xiashen-format-converter/DEVELOPMENT.md`：§11 + 更新日期
- 构建产物：`plugin.wasm`、`xiashen-format-converter-0.1.0.rgplugin`（36.4 KB）

**宿主**：无改动（`layer.remove` / `host.layers` / `edit` 权限 / 单步 undo 全部复用既有能力）

**文档**：本文档

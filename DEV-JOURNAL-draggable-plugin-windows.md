# 开发实录：虾神插件弹窗支持鼠标拖动

> 2026-10-14。目标：虾神插件（xiashen-format-converter）几个工具弹出的窗口
> （「格式转换」表单、「图层信息」表格）能用鼠标拖动到任意位置。
>
> 结论先行：**egui 里窗口拖不动几乎都是 `anchor()` 造成的**——它内部调用
> `movable(false)`。把 `.anchor(Align2::CENTER_CENTER, …)` 换成
> `.pivot(Align2::CENTER_CENTER).default_pos(…)` 即可：打开时仍然居中，标题栏恢复可拖。
> 全程只改 `crates/ui-egui/src/dialogs.rs` 两个函数，各 2 行。

---

## 1. 需求拆解

| 需求 | 落点 |
|---|---|
| 「格式转换」弹窗可拖动 | `show_form()`（插件命令的 `form` 表单对话框） |
| 「图层信息」弹窗可拖动 | `show_plugin()`（`host.dialog` 打开的表格窗口） |
| 拖动后不丢失 | egui 默认把窗口约束在 `content_rect` 内，无需额外处理 |

注意范围：**两个弹窗都在宿主侧**（`crates/ui-egui/src/dialogs.rs`），插件本身
（`plugins/xiashen-format-converter/`）一个字都不用改——弹窗是宿主画的，
插件只发请求。这与上一篇实录（`DEV-JOURNAL-host-dialog-and-layer-info.md`）
的结论一致："弹窗"是宿主的职责。

---

## 2. 排查过程

### 2.1 找到弹窗代码

`plugins/xiashen-format-converter` 的三个命令里有两个会弹窗：

| 命令 | 弹窗路径 | 宿主函数 |
|---|---|---|
| `convert`（格式转换） | `plugin.json` 的 `command.form` → `RustGisApp::run` 见到 form 就开表单 | `show_form()` |
| `layer-info`（图层信息） | 插件 `host.dialog` → `Session.pending_dialog` → UI 排水 | `show_plugin()` |
| `clear-layers`（移除所有图层） | 不弹窗（状态栏通知） | — |

### 2.2 为什么拖不动：读 egui 源码

两个函数都用 `.anchor(Align2::CENTER_CENTER, vec2(0.0, -40.0))` 定位。直觉上
"egui::Window 默认可拖"，但拖不动。没有猜，直接翻 egui 0.36.2 源码
（本机 registry：`E:\workspace\rust\.cargo\registry\src\rsproxy.cn-e3de039b2554c837\egui-0.36.2\`）：

```rust
// src/containers/area.rs:334
pub fn anchor(mut self, align: Align2, offset: impl Into<Vec2>) -> Self {
    self.anchor = Some((align, offset.into()));
    self.movable(false)          // ← 罪魁祸首
}
```

文档注释也写明（`area.rs:330`）："Anchoring also makes the window immovable."
即：**anchor 和 movable 互斥**，锚定了就不能拖。这不是 bug，是设计——被 anchor
的窗口位置每帧由锚点重算，拖了也会被拉回去，所以 egui 直接禁拖。

### 2.3 找本仓库的既有解法

工程里「插件管理器」窗口（`pluginsui.rs:806-813`）早就解决过同样的问题
（既要居中又要可拖），照抄它的写法：

```rust
.pivot(Align2::CENTER_CENTER)
.default_pos(screen.center())
```

- `pivot` 只是改 `default_pos`/`fixed_pos` 的对齐基准（把窗口的哪个点放到给定坐标），
  **不碰** `movable`；
- `default_pos` 只是**初始**位置，之后 egui 记住用户拖到哪（存在 `Memory::area_rect`）。

---

## 3. 实施

`crates/ui-egui/src/dialogs.rs`，两个函数同样的改法：

```rust
// 改前（anchor 钉死，不可拖）
.anchor(Align2::CENTER_CENTER, vec2(0.0, -40.0))

// 改后（居中打开，标题栏可拖）
.pivot(Align2::CENTER_CENTER)
.default_pos(ctx.content_rect().center() + vec2(0.0, -40.0))
```

细节：

1. **居中基准用 `ctx.content_rect()`** 而不是 `screen_rect()`：content_rect 是去掉
   功能区/面板后的中央区域，与 anchor 时代的视觉位置一致（`chrome.rs`、`pluginsui.rs`
   也都是这么算的）。`+ vec2(0.0, -40.0)` 保留原来向上偏 40px 的手感。
2. **拖动方式是 egui 默认的 `WindowDrag::OnTouch`**：非触摸屏 = 只有标题栏可拖
   （`window.rs:39`）。这正是想要的——窗口内容区有表格和滚动条，"随处可拖"
   会干扰文本选择和滚动。
3. **约束保留**：egui 默认 `constrain(true)`，窗口拖不出 `content_rect`，不会拖丢。

改动就这两处，各 2 行，外加一条中文注释说明"为什么不用 anchor"。

---

## 4. 踩过的坑

### 4.1 别把"预先存在的失败"当成自己改坏的

改完跑全套冒烟测试，5 个失败（功能区滚动、插件安装、插件页签……）。这些和弹窗
毫无关系，但必须证实而不是猜。**用 `git stash` 对照**：

```powershell
git stash                          # 摘掉本次改动
cargo test -p rustgis-ui-egui --test smoke   # 同样 5 个失败
git stash pop                      # 改动还在，且没丢
```

stash 前后同样失败 → 与本次改动无关。再查根因：正是上一篇实录 §4.7 记录过的
**中文环境性失败**——集成测试编译的是库的非 test 版本，`i18n::system_lang()` 读了
系统 locale，界面文案变中文，测试里的英文标签查不到。设英文环境重跑：

```powershell
$env:RUSTGIS_LANGUAGE="en"; cargo test -p rustgis-ui-egui --test smoke
# test result: ok. 9 passed; 0 failed
```

9/9 全过。**教训**：CI/测试挂了先 `git stash` 对照，再决定修不修；
本仓库在中文机上跑 UI 测试前先设 `RUSTGIS_LANGUAGE=en`。

### 4.2 本次目标测试是插件弹窗测试，必须单独确认

全套里覆盖本次改动的是 `a_plugin_window_shows_its_table`（上一篇实录加的），
单独跑一遍确认它过（改动前后都过——它只查窗口内容，不查位置，所以"改定位方式"
不破坏它，这也是它该有的行为）。

### 4.3 旧补丁文件里还有 anchor 写法

`plugins/xiashen-format-converter/patches/0001-plugin-command-forms.patch` 是给
"没有 form 特性的旧宿主"准备的补丁，里面 `show_form` 还是 `.anchor(...)` 版本。
该补丁在当前工作区本来就无法 `git apply`（form 特性已应用，行号全对不上），
所以本次**没有动它**——但如果哪天要用它装到旧宿主上，装完记得把那两行改成
`pivot + default_pos`，否则旧宿主上弹窗依旧不可拖。

---

## 5. 验证

| 项 | 命令 | 结果 |
|---|---|---|
| 编译 | `cargo check -p rustgis-ui-egui` | ✓ |
| 静态检查 | `cargo clippy -p rustgis-ui-egui --all-targets` | ✓ 无警告 |
| 目标测试 | `cargo test -p rustgis-ui-egui --test smoke a_plugin_window_shows_its_table` | ✓ |
| 全套冒烟（英文环境） | `RUSTGIS_LANGUAGE=en cargo test -p rustgis-ui-egui --test smoke` | ✓ 9/9 |
| wasm 不破坏 | `cargo check --target wasm32-unknown-unknown -p rustgis-web` | ✓ |

手动验证（`cargo run --release -p rustgis`）：点「格式转换」/「图层信息」→
窗口居中弹出 → 按住标题栏拖动 → 松手停在原地，再开仍是可拖的。

---

## 6. 经验教训（可复用的）

1. **"X 做不到"时先翻库源码，别猜**。egui 的 `anchor()` 静默地把 `movable` 关掉，
   文档只有一行小字。一条 `grep "pub fn anchor" <registry>/egui-*/src` 比十次试错快。
2. **居中且可拖 = `pivot` + `default_pos`，不是 `anchor`**。egui 的定位方式有一张
   隐含的能力表：

   | 定位方式 | 可拖 | 可调大小 |
   |---|---|---|
   | `anchor(…)` | ✗（强制 movable(false)） | 看 `resizable` |
   | `fixed_pos(…)` | ✗ | 看 `resizable` |
   | `pivot(…)` + `default_pos(…)` | ✓（记住拖动后的位置） | ✓ |

3. **先找工程里的既有解法**。「插件管理器」窗口早就是 pivot + default_pos，
   照抄既保证一致，也省去重新论证。GUI 工程里"别的窗口怎么做的"就是风格指南。
4. **对照实验用 `git stash`，不要口头断言**。"这个失败和我无关"必须有证据：
   stash 前后失败集合相同，才是无关；然后再用环境变量分清"环境性"和"真回归"。
5. **改的是宿主弹窗，插件零改动**。又一次印证分层：插件只发请求，界面归宿主。
   想让所有插件（不止虾神）的弹窗都可拖，就改宿主那一处。
6. **PowerShell 把 cargo 的 stderr 当错误**：成功时也报 exit code 1，
   看输出里的 `Finished` / `test result: ok`，别看 `$LASTEXITCODE`
   （与上一篇实录 §4.8 相同，再次踩到）。

---

## 附：改动文件清单

- `crates/ui-egui/src/dialogs.rs`：
  - `show_plugin()`：`.anchor(...)` → `.pivot(...)` + `.default_pos(...)`
  - `show_form()`：同上
- 本文档

（插件目录、引擎、i18n 均未改动。）

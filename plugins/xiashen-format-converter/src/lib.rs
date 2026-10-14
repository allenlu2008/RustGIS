//! 虾神格式转换（xiashen-format-converter）—— RustGIS 桌面插件。
//!
//! 功能：在独立的“虾神格式转换”选项卡上点“格式转换”，弹出对话框选择本机的
//! Shapefile（.shp），执行后读入地图、转换为 GeoJSON 写到同目录，再移除临时图层。
//! 同一选项卡上还有“图层信息”（弹窗列出内容列表）与“移除所有图层”（清空内容列表）。
//!
//! 本插件按 `docs/plugins.md` 的 ABI（版本 1）**直接手写宿主胶水**（crates.io 上没有
//! `rustgis-plugin` SDK crate）：
//! - 导出 `memory`、`rustgis_alloc(len)` 和 `rustgis_call(ptr, len)`
//!   （回复打包为 `ptr << 32 | len`）；
//! - 导入模块 `rustgis` 的 `call` / `result` / `log`，以 JSON 收发请求。
//!
//! 只能编译到 `wasm32-unknown-unknown`。宿主每次请求新建一个实例，因此这里不保留
//! 状态、也无需回收内存；非 wasm 目标下本库为空（便于在宿主平台跑集成测试）。

#[cfg(target_arch = "wasm32")]
mod wasm {
    use serde_json::{Value, json};
    use std::slice;

    // 宿主提供的三个函数（模块 `rustgis`，ABI 版本 1）。
    #[link(wasm_import_module = "rustgis")]
    unsafe extern "C" {
        fn call(ptr: i32, len: i32) -> i32;
        fn result(ptr: i32);
        fn log(ptr: i32, len: i32);
    }

    /// 宿主要求的分配函数：返回 `len` 字节可写内存。请求是一次性的，不回收。
    #[unsafe(no_mangle)]
    pub extern "C" fn rustgis_alloc(len: i32) -> i32 {
        if len <= 0 {
            return 0;
        }
        let mut buf: Vec<u8> = Vec::with_capacity(len as usize);
        let ptr = buf.as_mut_ptr();
        std::mem::forget(buf);
        ptr as i32
    }

    /// 宿主的入口：处理 `ptr..ptr+len` 处的请求，返回 `回复指针 << 32 | 回复长度`。
    /// 回复是 `{"ok": …}` 或 `{"error": …}`。
    #[unsafe(no_mangle)]
    pub extern "C" fn rustgis_call(ptr: i32, len: i32) -> i64 {
        let reply = handle(ptr, len).unwrap_or_else(|e| json!({"error": e}).to_string().into_bytes());
        let mut boxed = reply.into_boxed_slice();
        let n = boxed.len() as u64;
        let p = boxed.as_mut_ptr() as u64;
        std::mem::forget(boxed);
        (((p & 0xFFFF_FFFF) << 32) | (n & 0xFFFF_FFFF)) as i64
    }

    /// 读宿主写入的请求并分发。`ptr`/`len` 由宿主的运行时给出（它先 `rustgis_alloc`
    /// 再写入），与插件的敌手方向相反，故直接按给定范围读取。
    fn handle(ptr: i32, len: i32) -> Result<Vec<u8>, String> {
        let n = usize::try_from(len).map_err(|_| "请求长度超出范围".to_string())?;
        if ptr <= 0 || n == 0 {
            return Err("空请求".into());
        }
        let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, n) };
        let req: Value = serde_json::from_slice(bytes).map_err(|e| format!("无法读取请求: {e}"))?;
        let kind = req.get("kind").and_then(Value::as_str).unwrap_or_default();
        let id = req.get("id").and_then(Value::as_str).unwrap_or_default();
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        let lang = req.get("lang").and_then(Value::as_str).unwrap_or("en");
        let out = match kind {
            "command" => command(id, &params, lang),
            other => Err(format!("不支持的请求类型 '{other}'")),
        };
        let reply = match out {
            Ok(v) => json!({"ok": v}),
            Err(e) => json!({"error": e}),
        };
        Ok(reply.to_string().into_bytes())
    }

    /// 命令表：`plugin.json` 里声明的每个命令 id 对应一个处理函数。
    fn command(id: &str, params: &Value, lang: &str) -> Result<Value, String> {
        match id {
            "convert" => convert(params, lang),
            "layer-info" => layer_info(params, lang),
            "clear-layers" => clear_layers(lang),
            other => Err(format!("未知命令 '{other}'")),
        }
    }

    /// 向宿主发一个请求（命令 id 或 `host.*`），取回 `ok` 值。
    fn host_call(op: &str, params: &Value) -> Result<Value, String> {
        let req = json!({"op": op, "params": params}).to_string().into_bytes();
        let n = unsafe { call(req.as_ptr() as i32, req.len() as i32) };
        if n <= 0 {
            return Err(format!("{op}: RustGIS 没有回复"));
        }
        let mut buf = vec![0u8; n as usize];
        unsafe { result(buf.as_mut_ptr() as i32) };
        let reply: Value = serde_json::from_slice(&buf).map_err(|e| format!("{op}: 无法解析回复: {e}"))?;
        if let Some(e) = reply.get("error") {
            return Err(match e.as_str() {
                Some(s) => s.to_string(),
                None => e.to_string(),
            });
        }
        Ok(reply.get("ok").cloned().unwrap_or(Value::Null))
    }

    /// 一行日志（宿主的 log）。
    fn log_line(text: &str) {
        unsafe { log(text.as_ptr() as i32, text.len() as i32) };
    }

    /// 按界面语言取消息（`lang` 为 `zh`、`zh-CN`… 时用中文）。
    fn tr(lang: &str, zh: &str, en: &str) -> String {
        if lang.starts_with("zh") { zh.to_string() } else { en.to_string() }
    }

    /// `C:\data\demo.shp` → `C:\data\demo.geojson`（调用方已确认以 .shp 结尾）。
    fn out_path(path: &str) -> String {
        match path.rfind('.') {
            Some(i) if i > 0 => format!("{}.geojson", &path[..i]),
            _ => format!("{path}.geojson"),
        }
    }

    /// 主流程：读入 Shapefile → 导出 GeoJSON → 移除临时图层 → 通知结果。
    fn convert(params: &Value, lang: &str) -> Result<Value, String> {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                tr(
                    lang,
                    "尚未选择文件：请在弹出的对话框里选择 .shp 文件（若按钮没有弹窗，请先应用插件目录 patches/ 下的宿主补丁）",
                    "no file chosen: pick a .shp file in the dialog (apply the host patch from the plugin's patches/ folder if the button opened no dialog)",
                )
            })?;
        if !path.to_ascii_lowercase().ends_with(".shp") {
            return Err(tr(lang, "只支持 Shapefile（.shp）格式的文件", "only Shapefile (.shp) files are supported"));
        }
        let out = out_path(path);

        // 1) 读入本机 shp（layer.addData），拿到临时图层 id。
        log_line(&format!("读取 {path}"));
        let added = host_call("layer.addData", &json!({"path": path}))?;
        let layer = added.get("layer").cloned().unwrap_or(Value::Null);
        if layer.is_null() {
            return Err(tr(lang, &format!("无法读取 {path}"), &format!("could not read {path}")));
        }

        // 2) 导出 GeoJSON；3) 无论成败都移除临时图层（失败则由宿主整体回滚）。
        let exported = host_call("share.exportLayer", &json!({"layer": layer, "path": &out}));
        let removed = host_call("layer.remove", &json!({"layer": &layer}));
        let done = match (exported, removed) {
            (Ok(d), Ok(_)) => d,
            (Err(e), _) => return Err(e),
            (Ok(_), Err(e)) => return Err(e),
        };
        let features = done.get("features").and_then(Value::as_u64).unwrap_or(0);

        // 4) 状态栏通知。
        log_line(&format!("已写入 {out}（{features} 个要素）"));
        let text = tr(
            lang,
            &format!("已转换为 GeoJSON：{out}（{features} 个要素）"),
            &format!("Converted to GeoJSON: {out} ({features} features)"),
        );
        host_call("host.notify", &json!({"text": text}))?;
        Ok(json!({"input": path, "output": out, "features": features}))
    }

    /// 每个几何类型的中文名（`host.layers` 的 `geometry` 是 Rust 的 Debug 文本）。
    fn geometry_zh(g: &str) -> &str {
        match g {
            "Point" => "点",
            "Polyline" => "线",
            "Polygon" => "面",
            "None" => "表格",
            other => other,
        }
    }

    /// “图层信息”：读宿主的 `host.layers`，把每个图层的名称、几何类型、路径、要素数、
    /// 字段数与可见性排成表格，交给宿主的 `host.dialog` 弹窗展示。
    fn layer_info(_params: &Value, lang: &str) -> Result<Value, String> {
        let layers = host_call("host.layers", &json!({}))?;
        let list = layers.as_array().map(Vec::as_slice).unwrap_or(&[]);
        let zh = lang.starts_with("zh");
        let columns = if zh {
            vec!["名称", "几何类型", "路径", "要素数", "字段数", "可见"]
        } else {
            vec!["Name", "Geometry", "Path", "Features", "Fields", "Visible"]
        };
        let rows: Vec<Vec<Value>> = list
            .iter()
            .map(|l| {
                let g = l.get("geometry").and_then(Value::as_str).unwrap_or_default();
                let geom = if l.get("raster").and_then(Value::as_bool) == Some(true) {
                    tr(lang, "栅格", "Raster")
                } else if zh {
                    geometry_zh(g).to_string()
                } else if g == "None" {
                    "Table".to_string()
                } else {
                    g.to_string()
                };
                // 路径：本机文件的路径；工程内的图层没有路径。
                let path = match l.get("path").and_then(Value::as_str) {
                    Some(p) if !p.is_empty() => Value::String(p.to_string()),
                    _ => Value::String(tr(lang, "工程内", "In project")),
                };
                let visible =
                    if l.get("visible").and_then(Value::as_bool) == Some(false) { tr(lang, "否", "No") } else { tr(lang, "是", "Yes") };
                vec![
                    l.get("name").cloned().unwrap_or(Value::Null),
                    Value::String(geom),
                    path,
                    l.get("features").cloned().unwrap_or(Value::Null),
                    json!(l.get("fields").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0)),
                    Value::String(visible),
                ]
            })
            .collect();
        if list.is_empty() {
            return Err(tr(lang, "内容列表里还没有图层", "the content list has no layers yet"));
        }
        // 标题与备注也交给宿主裁剪，插件只负责给一次能读完的信息。
        let title = tr(lang, "图层信息", "Layer information");
        let note = tr(lang, &format!("共 {} 个图层", list.len()), &format!("{} layers in all", list.len()));
        host_call("host.dialog", &json!({"title": title, "columns": columns, "rows": rows, "note": note}))?;
        Ok(json!({"layers": list.len()}))
    }

    /// “移除所有图层”：先用 `host.layers` 拿到内容列表的**快照**（图层 id 稳定，删除过程中
    /// 列表会变，所以必须先取全再删），再逐个发 `layer.remove`。任何一步失败都直接返回
    /// `Err`——宿主会把整个命令回滚，不会留下删了一半的工程；整个命令是**一个撤销步骤**。
    fn clear_layers(lang: &str) -> Result<Value, String> {
        let layers = host_call("host.layers", &json!({}))?;
        let list = layers.as_array().map(Vec::as_slice).unwrap_or(&[]);
        if list.is_empty() {
            return Err(tr(lang, "内容列表里还没有图层", "the content list has no layers yet"));
        }
        // 优先用 id（数字），宿主找不到 id 时才退回名字；两者都缺的图层无法移除，报错而不是跳过。
        let targets: Vec<Value> = list
            .iter()
            .map(|l| match l.get("id") {
                Some(id) if !id.is_null() => id.clone(),
                _ => l.get("name").cloned().unwrap_or(Value::Null),
            })
            .collect();
        if targets.iter().any(Value::is_null) {
            return Err(tr(lang, "有的图层没有 id 或名称，无法移除", "a layer has neither an id nor a name to remove it by"));
        }

        let mut removed = 0usize;
        for target in &targets {
            log_line(&format!("移除图层 {target}"));
            host_call("layer.remove", &json!({"layer": target}))?;
            removed += 1;
        }

        let text = tr(
            lang,
            &format!("已从当前工程移除全部 {removed} 个图层"),
            &format!("Removed all {removed} layers from the project"),
        );
        host_call("host.notify", &json!({"text": text}))?;
        Ok(json!({"removed": removed}))
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm::*;

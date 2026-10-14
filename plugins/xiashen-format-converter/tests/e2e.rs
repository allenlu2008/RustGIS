//! 端到端测试：把 `plugin.json` 与编译出的 `plugin.wasm` 装进宿主的 `rustgis-plugins`，
//! 用模拟的 Host 走一遍完整执行链（不启动 GUI；弹窗由宿主补丁提供，见 `patches/`）。

use std::path::{Path, PathBuf};

use rustgis_plugins::{Host, Manifest, Package};
use serde_json::{Value, json};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// 模拟宿主：记录每个请求，并按 RustGIS 的真实回复作答。
#[derive(Default)]
struct Mock {
    ops: Vec<(String, Value)>,
    #[allow(dead_code)]
    logs: Vec<String>,
    /// 内容列表为空：`host.layers` 返回空数组。
    empty: bool,
}

impl Host for Mock {
    fn call(&mut self, op: &str, params: &Value) -> Result<Value, String> {
        self.ops.push((op.to_string(), params.clone()));
        match op {
            "layer.addData" => {
                let p = params.get("path").and_then(Value::as_str).unwrap_or_default();
                assert!(p.to_ascii_lowercase().ends_with(".shp"), "layer.addData 应收到 .shp 路径，得到 {p}");
                Ok(json!({"layer": 7, "features": 3}))
            }
            "share.exportLayer" => {
                assert_eq!(params.get("layer"), Some(&json!(7)), "导出应使用 addData 返回的图层 id");
                let p = params.get("path").and_then(Value::as_str).unwrap_or_default();
                assert!(p.ends_with(".geojson"), "输出应是 .geojson，得到 {p}");
                Ok(json!({"path": p, "features": 3, "leftOut": 0}))
            }
            "layer.remove" => Ok(Value::Null),
            "host.notify" => Ok(Value::Null),
            "host.layers" => {
                if self.empty {
                    return Ok(json!([]));
                }
                Ok(json!([
                    {"id": 1, "name": "Cities", "raster": false, "visible": true, "path": r"C:\data\cities.shp",
                     "geometry": "Point", "features": 3, "selected": 0, "active": true, "fields": [{"name": "name", "type": "Text"}]},
                    {"id": 2, "name": "Background", "raster": true, "visible": false, "path": null,
                     "geometry": "None", "features": 0, "selected": 0, "active": false, "fields": []},
                ]))
            }
            "host.dialog" => {
                let title = params.get("title").and_then(Value::as_str).unwrap_or_default();
                assert!(!title.is_empty(), "host.dialog 应带标题");
                Ok(Value::Null)
            }
            other => Err(format!("Mock 没有实现 '{other}'")),
        }
    }

    fn log(&mut self, text: &str) {
        self.logs.push(text.to_string());
    }
}

/// 加载插件（未构建 `plugin.wasm` 时跳过测试）。
fn load() -> Option<std::sync::Arc<rustgis_plugins::Plugin>> {
    let manifest_text = std::fs::read_to_string(dir().join("plugin.json")).expect("应能读取 plugin.json");
    let wasm = match std::fs::read(dir().join("plugin.wasm")) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("跳过：plugin.wasm 还没构建（先运行 build.ps1）");
            return None;
        }
    };
    let manifest = Manifest::parse(&manifest_text).expect("plugin.json 应通过宿主校验");
    let pkg = Package { manifest, wasm, sha256: "test".to_string() };
    rustgis_plugins::load(pkg, "test").ok()
}

#[test]
fn converts_a_shapefile_to_geojson_end_to_end() {
    let Some(_plugin) = load() else { return };
    let cmd = rustgis_plugins::command("plugin:xiashen-format-converter/convert").expect("命令应已注册");
    // 应用了宿主补丁时，form 才会被解析进命令声明；未应用时该字段不存在（向后兼容）。
    // 用序列化后是否存在 "form" 键来判断，避免在未打补丁的宿主上引用编译期不存在的字段。
    let decl: Value = serde_json::to_value(&cmd.decl).expect("命令声明应可序列化");
    if let Some(form) = decl.get("form") {
        let arr = form.as_array().expect("form 应是数组");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("kind").and_then(Value::as_str), Some("path"));
        assert_eq!(arr[0].get("required").and_then(Value::as_bool), Some(true));
        assert_eq!(arr[0].get("accept").and_then(Value::as_array).map(|a| a.len()), Some(1));
        assert_eq!(arr[0]["accept"][0].as_str(), Some("shp"));
    }

    // 正常路径：读 shp → 导出同目录 .geojson → 移除临时图层 → 通知。
    let mut host = Mock::default();
    let v = rustgis_plugins::run_command(&cmd, &mut host, &json!({"path": r"C:\data\demo.shp"})).expect("应转换成功");
    assert_eq!(v.get("output").and_then(Value::as_str), Some(r"C:\data\demo.geojson"));
    assert_eq!(v.get("input").and_then(Value::as_str), Some(r"C:\data\demo.shp"));
    assert_eq!(v.get("features").and_then(Value::as_u64), Some(3));
    let ops: Vec<&str> = host.ops.iter().map(|(o, _)| o.as_str()).collect();
    assert_eq!(ops, ["layer.addData", "share.exportLayer", "layer.remove", "host.notify"]);

    // 没有补丁时按钮直接运行：params 为空 → 给出可执行的错误提示，而不是碰宿主命令。
    let mut host = Mock::default();
    let e = rustgis_plugins::run_command(&cmd, &mut host, &Value::Null).expect_err("缺少 path 应报错");
    assert!(!host.ops.iter().any(|(o, _)| o == "layer.addData"), "失败时不应运行宿主命令");
    assert!(e.contains(".shp"), "错误应指引选择文件：{e}");

    // 只接受 .shp。
    let mut host = Mock::default();
    let e = rustgis_plugins::run_command(&cmd, &mut host, &json!({"path": r"C:\data\demo.txt"})).expect_err("非 shp 应报错");
    assert!(e.contains("shp"), "{e}");
    assert!(host.ops.is_empty(), "非 shp 时不应碰宿主");
}

/// “图层信息”：读宿主图层列表，把每个图层排成表格交给宿主弹窗。
#[test]
fn lists_the_layers_in_a_dialog_end_to_end() {
    let Some(_plugin) = load() else { return };
    let cmd = rustgis_plugins::command("plugin:xiashen-format-converter/layer-info").expect("layer-info 命令应已注册");
    let mut host = Mock::default();
    // 界面是中文时列头与取值也应是中文（宿主的 lang 由测试环境决定，故只断言结构性内容）。
    let v = rustgis_plugins::run_command(&cmd, &mut host, &Value::Null).expect("应能列出图层");
    assert_eq!(v.get("layers").and_then(Value::as_u64), Some(2));
    let ops: Vec<&str> = host.ops.iter().map(|(o, _)| o.as_str()).collect();
    assert_eq!(ops, ["host.layers", "host.dialog"]);
    // 表格：6 列 × 2 行，每行列数一致；栅格图层没有路径 → “工程内/In project”。
    let dialog = &host.ops[1].1;
    let columns = dialog.get("columns").and_then(Value::as_array).expect("应有列头");
    assert_eq!(columns.len(), 6, "列头应有 6 列: {columns:?}");
    let rows = dialog.get("rows").and_then(Value::as_array).expect("应有数据行");
    assert_eq!(rows.len(), 2, "应有 2 行: {rows:?}");
    for row in rows {
        assert_eq!(row.as_array().map(Vec::len), Some(6), "每行列数应与列头一致: {row:?}");
    }
    assert_eq!(rows[0][0].as_str(), Some("Cities"), "第一行应是第一个图层");
    let path = rows[0][2].as_str().unwrap_or_default();
    assert!(path.contains("cities.shp"), "文件图层应显示其路径: {path}");
    assert!(rows[1][2].as_str().is_some_and(|p| p == "工程内" || p == "In project"), "工程内图层的路径应是占位文本: {:?}", rows[1][2]);
    let note = dialog.get("note").and_then(Value::as_str).unwrap_or_default();
    assert!(note.contains('2'), "备注应说明共 2 个图层: {note}");
}

/// “移除所有图层”：先读内容列表快照，再逐个 `layer.remove`，最后状态栏通知。
#[test]
fn removes_every_layer_end_to_end() {
    let Some(_plugin) = load() else { return };
    let cmd = rustgis_plugins::command("plugin:xiashen-format-converter/clear-layers").expect("clear-layers 命令应已注册");
    let mut host = Mock::default();
    let v = rustgis_plugins::run_command(&cmd, &mut host, &Value::Null).expect("应能移除所有图层");
    assert_eq!(v.get("removed").and_then(Value::as_u64), Some(2));
    let ops: Vec<&str> = host.ops.iter().map(|(o, _)| o.as_str()).collect();
    assert_eq!(ops, ["host.layers", "layer.remove", "layer.remove", "host.notify"]);
    // 按快照顺序、用 id 逐个移除（栅格图层也一并移除）。
    assert_eq!(host.ops[1].1, json!({"layer": 1}));
    assert_eq!(host.ops[2].1, json!({"layer": 2}));
    let note = host.ops[3].1.get("text").and_then(Value::as_str).unwrap_or_default();
    assert!(note.contains('2'), "通知应说明移除了 2 个图层: {note}");

    // 内容列表为空：只读一次 host.layers，不碰任何改动命令，给出可读的错误。
    let mut host = Mock { empty: true, ..Mock::default() };
    let e = rustgis_plugins::run_command(&cmd, &mut host, &Value::Null).expect_err("没有图层应报错");
    assert!(e.contains("图层") || e.contains("layers"), "{e}");
    assert_eq!(host.ops.len(), 1, "空列表时不应尝试移除: {:?}", host.ops);
}

#[test]
fn the_package_reads_back_as_a_zip() {
    let pkg = dir().join("xiashen-format-converter-0.1.0.rgplugin");
    let bytes = match std::fs::read(&pkg) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("跳过：还没运行 build.ps1 打包");
            return;
        }
    };
    let p = Package::from_zip(&bytes).expect("包应是可读的 zip（条目名用正斜杠）");
    assert_eq!(p.manifest.id, "xiashen-format-converter");
    assert_eq!(p.manifest.commands[0].id, "convert");
    assert!(!p.wasm.is_empty(), "包内应有 wasm 模块");
    let plugin = rustgis_plugins::load(p, "zip").expect("包应能加载");
    assert!(plugin.enabled);
}

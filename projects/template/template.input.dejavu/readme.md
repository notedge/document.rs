# template-input-dejavu

`template.input.dejavu`：Dejavu 模板语言，登记为 `template.input.dejavu`。

- 语法：`.doki` 用 `{% %}` / `{# #}`，`.dejavu` 与其它扩展名用 `<% %>` / `<# #>`；每个
  来源按自身扩展名解析，跨扩展名 include / extends / import 按被引用来源解析。
- 输出：HTML。`DejavuEngine` 实现共享 `TemplateEngine`（`id = "dejavu"`，
  `contract = "dejavu/v0"`，`extensions = ["", ".doki", ".dejavu", ".html"]`）。
- 诊断：引擎专有失败继续发 `dejavu.template.*` 冻结码，不改写成 `template.input.dejavu.*`；
  共享 loader / 预算失败仍走 `template.*`。
- 同时提供 IR 级入口（`parse` / `render_ir` 等），供 AOT codegen 与既有宿主复用。

```rust,ignore
use template::{MapTemplateLoader, RenderOptions, RenderRequest, TemplateRoot};
use template_input_dejavu::DejavuEngine;
use serde_json::json;

let loader = MapTemplateLoader::new().with_root(
    TemplateRoot::new("theme", 0).with_file("page.doki", "{% page.title %}"),
);
let output = DejavuEngine.render(&RenderRequest {
    source: "{% include \"page.doki\" %}",
    entry: None,
    loader: &loader,
    data: &json!({ "page": { "title": "Hello" } }),
    options: &RenderOptions::default(),
})?;
```

数据统一是 `serde_json::Value`；缺失与 `null` 在取值层面等价，strict 模式下缺失才报错。

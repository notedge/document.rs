# template

模板引擎合同。它只负责**模板源 → 文本**：定义类型、loader、预算、诊断与 `TemplateEngine`
接口，不持有文档语义，也不进入任何格式路由。

具体语法在子命名空间：

| 目录                            | Package                         | 语法                    |
|---------------------------------|---------------------------------|-------------------------|
| `template`             | `template`             | 合同（本 crate）        |
| `template.input.hbs`   | `template-input-hbs`   | Handlebars 语法子集     |
| `template.input.jinja` | `template-input-jinja` | Jinja2 语法子集         |

```rust,ignore
use template::{MapTemplateLoader, RenderOptions, TemplateId, TemplateRoot};
use serde_json::json;

let loader = MapTemplateLoader::new().with_root(
    TemplateRoot::new("map", 0).with_file("hello.hbs", "Hello {{name}}"),
);
let data = json!({ "name": "world" });
let output = template_input_hbs::HbsEngine.render(&template::RenderRequest {
    source: "{{> hello}}",
    entry: Some(&TemplateId::new("map", "main.hbs")),
    loader: &loader,
    data: &data,
    options: &RenderOptions::default(),
})?;
assert_eq!(output.text, "Hello world");
```

渲染结果由调用方按 `capability().content` 交给既有输入层（HTML 用
`document.formats::import::html`），再走 `document-convert` 的现有路由。本 crate
不产出 `DocumentGraph`，不定义第二套文档语义。

失败是 fail-closed：不支持的构造、越限、引用解析失败都拒绝整次渲染并给稳定诊断码
（`template.*` / `template.input.<engine>.*`），不静默丢弃内容。

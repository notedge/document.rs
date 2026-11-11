# document-template-input-hbs

`document.template.input.hbs`：Handlebars 语法子集，输出 HTML。

支持插值（`{{expr}}` 转义、`{{{expr}}}` 原样）、`#if` / `#unless` / `#each` / `#with`、
`else` / `else if`、partial（`{{> ref}}`）、注释、`~` 空白控制与 `@root` / `@index` /
`@key` / `@first` / `@last` / `../` 路径。helper 调用、未知块与 partial 参数一律
fail-closed，分别给 `template.input.hbs.unsupported-helper`、`template.input.hbs.unsupported-block`、
`template.input.hbs.unsupported-construct`。

v0 已知偏差：不做 handlebars.js 的 standalone-line 空白剥离；对象遍历按 key 升序。

```rust,ignore
use document_template::{MapTemplateLoader, RenderOptions, TemplateId, TemplateRoot};
use document_template_input_hbs::HbsEngine;
use serde_json::json;

let loader = MapTemplateLoader::new().with_root(
    TemplateRoot::new("map", 0).with_file("hello.hbs", "Hello {{name}}"),
);
let output = HbsEngine.render(&document_template::RenderRequest {
    source: "{{> hello}}",
    entry: Some(&TemplateId::new("map", "main.hbs")),
    loader: &loader,
    data: &json!({ "name": "world" }),
    options: &RenderOptions::default(),
})?;
```

数据统一是 `serde_json::Value`；真值按 JS 语义，字符串化按 JS 语义，转义集为
`& < > " ' \` =`。越限与缺失在 strict 模式下失败，不截断、不静默。

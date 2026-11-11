# template-framework-doki

`template.framework.doki`：Doki 构建框架的模板入口。

- 选择入口布局、按来源扩展名决定语言 profile；
- 宿主扩展（i18n `t()` registry 等）由调用方作为 `dejavu` 的 `CustomRegistry` 注入；
- 语法与渲染委托给 `template.input.dejavu`，本 crate 不重写 parser、转义或空白语义。

```rust,ignore
use serde_json::json;
use template_framework_doki::{layout_loader, DokiFramework};
use template_framework::{FrameworkRequest, TemplateFramework};
use template::RenderOptions;

let (loader, entry) = layout_loader("page.doki", "{% page.title %}");
let options = RenderOptions::default();
let framework = DokiFramework { custom: None };
let output = framework.render(&FrameworkRequest {
    entry: &entry.to_string(),
    loader: &loader,
    data: &json!({ "page": { "title": "Hello" } }),
    options: &options,
})?;
assert_eq!(output.text, "Hello");
```

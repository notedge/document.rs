# document-template-input-jinja

`document.template.input.jinja`：Jinja2 语法子集，输出 HTML。只做模板源 → 文本，不拥有文档语义。

支持 `{{ expr }}`（默认 HTML 转义）、`{% if %}` / `{% elif %}` / `{% else %}` / `{% endif %}`、
`{% for x in expr %}`（`loop.index` / `index0` / `first` / `last` / `length`，支持 `{% else %}`）、
`{% set name = expr %}`、`{% include "ref" %}`、`{# … #}` 注释，以及 `{{-` / `-}}`、`{%-` / `-%}`、
`{#-` / `-#}` 单侧空白控制。

表达式支持字面量、名字、`a.b`、`a[k]`、`+ - * / %`、`~` 连接、比较、`and` / `or` / `not` 与括号。
过滤器只支持 `upper`、`lower`、`trim`、`length`、`default(x)`、`join(sep)`、`replace(a, b)`、
`escape`、`safe`。

```rust,ignore
use document_template::{MapTemplateLoader, RenderOptions, RenderRequest, TemplateId, TemplateRoot};
use document_template_input_jinja::JinjaEngine;
use serde_json::json;

let loader = MapTemplateLoader::new().with_root(
    TemplateRoot::new("map", 0).with_file("hello.jinja", "Hello {{ name }}"),
);
let output = JinjaEngine.render(&RenderRequest {
    source: "{% include \"hello.jinja\" %}",
    entry: Some(&TemplateId::new("map", "main.jinja")),
    loader: &loader,
    data: &json!({ "name": "world" }),
    options: &RenderOptions::default(),
})?;
```

## 拒绝与偏差

不支持的标签（`extends` / `block` / `macro` / `call` / `filter` / `raw` / `with` / `import` /
`from`）给 `template.input.jinja.unsupported-tag`；`is` 测试、`for` / `set` 解构、`include` 的非常量
引用与 `for ... if ...` 循环过滤给 `template.input.jinja.unsupported-construct`。未知过滤器给
`template.input.jinja.unknown-filter`，参数个数不符给 `template.input.jinja.filter-arity`，类型不符给
`template.input.jinja.filter-value`，运算子类型不符给 `template.input.jinja.operator-type`，除零 / 取模零 /
整数溢出给 `template.input.jinja.arithmetic`，`for` 源不可迭代给 `template.input.jinja.iteration-type`。
一律拒绝整次渲染，不静默丢内容。

v0 已知偏差：容器字符串化按 Python 风格（`None`、`True` / `False`、列表 `[a, b]`、字典
`{'k': v}`，对象 key 升序）；比较按 JSON 结构相等，布尔与数字不互转；`default(x)` 只在缺失或
`null` 时替换；不实现 `trim_blocks` / `lstrip_blocks`，`{%+` 在默认关闭下等价于无标记并显式接受；
`for` 只接受数组与对象（对象遍历 key），字符串与标量给 `iteration-type`；只有 `safe` / `escape`
标记安全，其他过滤器产出一律重新转义。

数据统一是 `serde_json::Value`，缺失与 `null` 在取值层面等价，转义集为 `& < > " '`。

# template-input-dejavu-codegen

`template.input.dejavu.codegen`：Dejavu AOT 代码生成，IR → ESM JavaScript。

- `precompile_single`：单模板 IR → 一个 ESM 模块；
- `precompile_catalog`：catalog → 多个模块与 manifest；
- 每个生成构造携带行起点与模板 span，供宿主生成 source map；
- 不做解析与求值，输入输出都是 `template-input-dejavu` 的 IR 类型。

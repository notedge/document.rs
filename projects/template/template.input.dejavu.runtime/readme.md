# template-input-dejavu-runtime

`template.input.dejavu.runtime`：Dejavu AOT 产物使用的最小运行时 helper（`no_std`）。

- HTML / text escaper、`Template` trait、loop 与 display 辅助；
- 不解析模板、不持有 loader、不依赖 `template` 合同；
- 供 `template-input-dejavu` 的 AOT 产物与宿主运行时复用。

# projects/

本目录按产品域收纳 Rust 实现（**维护者索引**，不是产品落地页）。用户向说明见根 [`../readme.md`](../readme.md)，代理硬规则见
[`../AGENTS.md`](../AGENTS.md)。

| 目录                              | 品牌         | 架构说明                                                         |
|-----------------------------------|--------------|------------------------------------------------------------------|
| [`plotter/`](plotter/readme.md)   | `plotter-*`  | 绘图：GoG + Diagram/Chart，无 `plotter.core` / 万能 `plotter.ir` |
| [`document/`](document/readme.md) | `document-*` | 文档：IR → 转换 / 排版 / 渲染，字体逻辑与字节分离                |
| [`crawler/`](crawler/readme.md)   | `crawler-*`  | 采集：seed → fetch → extract → store（原 Pandark）               |

目录名用点分层，Cargo package 名用连字符。门面 crate 只做再导出；复杂 fixtures / golden / 合同 JSON 留在下游。

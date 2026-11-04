# AGENTS

本仓库是 **中文优先** 的文档族 Rust 聚合实现仓（`document.rs`）。代理动手前先读本文件与三个产品树 readme；不要只靠聊天记忆或过时的根
`readme.md` 模板。

## 仓库是什么

把 Notedown / Panduck / VuTeX / Crawler（原 Pandark）/ Plotter（原 Apollo + DaVinci）的 **Rust 实现** 收到同一
workspace，按产品域分目录：

| 目录                 | 域   | 说明                                                                     |
|----------------------|------|--------------------------------------------------------------------------|
| `projects/plotter/`  | 绘图 | GoG 链 + Diagram/Chart 矢量链；品牌 `plotter-*`                          |
| `projects/document/` | 文档 | IR / formats / convert / layout / renderer / 字体处理；品牌 `document-*` |
| `projects/crawler/`  | 采集 | 抓取编排 / fetch / extract / store；品牌 `crawler-*`（原 Pandark）       |

目录名用点分层（`plotter.input.mermaid`），Cargo package 名用连字符（`plotter-input-mermaid`）。

## 必读

1. [`projects/plotter/readme.md`](projects/plotter/readme.md) — Plotter 包图、为何没有 `plotter.ir` / `plotter.core`、
   两条主链与资产边界。
2. [`projects/document/readme.md`](projects/document/readme.md) — Document 包图、转换链与排版/渲染链、字体资产边界。
3. [`projects/crawler/readme.md`](projects/crawler/readme.md) — Crawler 包图、seed→fetch→extract→store、诊断 namespace。
4. 工作区外设计真相仍在 `E:\vmz 全栈开发\规划设计\notedown\`（尤其 `01`、`05`、`06`）与 `规划设计/davinci/`；
   实现与文档冲突时先改文档或明示冲突。
5. 下游产品仓（`E:\文档研究\` 下的 VuTeX / panduck / pandark / notedown 等）通过 **git `rev` pin** 消费本仓；本地用
   `.cargo/config.toml` 绝对路径 overlay，`.cargo/config.example.toml` 写占位路径。

## 硬规则

### 语言与提交

- 用户可见说明、仓内设计短文、模块文档注释： **中文**（标识符、crate 名、诊断 code 仍用英文稳定串）。
- Commit 标题：`<gitmoji> <中文简述>`，标识符用反引号；正文不强制。
- `1.0.0` 以前合同命名空间用 `/v0`；不承担旧 API/ABI/schema 兼容，直接改实现与测试。数据库迁移功能不受此禁令削减。

### 布局与品牌

- **禁止**再引入 `plotter.core`：GoG 原语并入 `plotter.types` 的 `plot` 模块。
- **禁止**新建「万能」`plotter.ir`：`PlotSpec` / `DiagramSpec` / Chart Spec 平行，共享停在 `plotter.types` +
  `plotter.scene`。
- `plotter.diagram` **只做** `DiagramSpec → plan → scene`；Mermaid / DOT / PlantUML 前端在 `plotter.input.*`。
- 门面 `plotter` / `document` / `crawler` 只允许 `pub mod xx { pub use …::*; }`（或同等再导出）形态，禁止业务逻辑塞进门面模块树；编排入口可留在
  `crawler` 根 API。

### 资产边界（实现仓干净）

本仓 **不进** 二进制字体、视觉 golden（PNG / SVG / fingerprint）、大型 fixtures：

| 内容                                         | 归属                                           |
|----------------------------------------------|------------------------------------------------|
| 字体解析 / metrics / variants 逻辑           | `document.text` 等处理 crate                   |
| `.otf` / `.ttf` 字节                         | **下游**（如 VuTeX，`install_math_font` 注入） |
| diagram/chart 像素黄金与 fingerprint         | **下游**或 CI artifact                         |
| 文本合同（如拒绝目录 JSON）                  | 可留在实现仓（`src` 合同，非测试套件）         |
| 数据驱动合同 JSON / 视觉 golden / 浏览器门禁 | **下游**；实现仓 tests 只留冒烟                |

改代码时不要把资产重新拷回 `projects/**`。

### 编码与工具

- 源文件 UTF-8 **无 BOM**。用编辑器/脚本改中文文件时禁止默认 ANSI/`Set-Content` 无编码，以免注释炸成乱码。
- 工具链见根目录 [`rust-toolchain.toml`](rust-toolchain.toml)（nightly + `rustfmt` / `clippy`）；格式见
  [`rustfmt.toml`](rustfmt.toml)。改完跑 `cargo fmt --all`。
- Windows 上不要用会吞 `{` 的粗糙字符串替换去改 `use path::{A, B}`；改完必须 `cargo check` 相关包。
- 不要擅自把 `projects/plotter` 再收成 `plotter.rs/crates` 这类目录大挪移，除非用户显式要求。

### Git

- 逻辑块完成后 commit；用户要求推送或修 CI 时再 `push` 到 `origin/dev`。
- 重构过程中的 `wip` 堆叠，在 CI 绿后按用户要求 squash 成一条产品语义标题。
- 不强制 push、不改 `git config`、不 `push --force` 到 main/master（除非用户明确要求且已授权）。

## 常用命令

```bash
cargo metadata --no-deps --format-version 1
cargo check -p plotter -p plotter-types -p plotter-diagram
cargo check -p document -p document-ir -p document-convert
cargo check -p crawler -p crawler-types -p crawler-fetch
cargo test -p plotter-diagram
cargo test -p plotter-input-mermaid
cargo test -p plotter-chart --features vega-lite
cargo test -p crawler-types
```

## 代理行为

- 先读本文件与三个产品树 `readme.md`，再改代码。
- 大改动（拆 crate、并类型、动门面）前用几句话说明意图；用户已拍板的（如 input 拆分、不要 core/ir、品牌迁 `document-*` /
  `crawler-*`）直接做。
- 不要把下游产品仓的 napi / TS / 官网资产塞进本仓「图省事」。

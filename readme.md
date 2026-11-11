# document.rs

文档族 **Rust 实现聚合仓**：把 Notedown / Panduck / VuTeX / Crawler（原 Pandark）/ Plotter（原 Apollo + DaVinci）共用的核心能力收进同一
workspace，供下游产品按 `git rev` pin 消费。

## 💡 这是什么？

面向「写文档 → 转换 / 排版 / 绘图 → 抓取补全」这条产品链的 **实现库**，不是终端用户 CLI，也不是官网。

| 你得到的                                                       | 你不会在这里找到的                                    |
|----------------------------------------------------------------|-------------------------------------------------------|
| 按产品域再导出的 `document-*` / `plotter-*` / `crawler-*` / `template-*` Rust API | 二进制字体、视觉 golden、大型站点 fixtures |
| 语义 IR、格式转换、排版渲染、图语言与 GoG 绘图                 | npm CLI / NAPI 包装（在下游产品仓）                   |
| 可 pin 的源码依赖与本地 path overlay                           | 旧品牌 `notedown-*` / `pandark-*` / `apollo` 合同兼容 |

实现仓 **中文优先**；crate 名、诊断 code、schema 字符串保持英文稳定标识。

## 🚀 怎么开始

### 1. 给代理 / 协作者（推荐）

先读根目录 [`AGENTS.md`](AGENTS.md)，再按任务打开对应产品树：

- 绘图 → [`projects/plotter/readme.md`](projects/plotter/readme.md)
- 文档 → [`projects/document/readme.md`](projects/document/readme.md)
- 采集 → [`projects/crawler/readme.md`](projects/crawler/readme.md)
- 模板 → [`projects/template/readme.md`](projects/template/readme.md)

### 2. 下游接入（手工）

在产品仓 `Cargo.toml` 用 git `rev` pin 本仓；本地开发可把 [`.cargo/config.example.toml`](.cargo/config.example.toml) 拷成
`.cargo/config.toml`，用绝对路径 overlay。

```toml
# 下游产品仓示例：`rev` 替换为本仓 `dev` 上的 40 位提交
document = { git = "https://github.com/notedge/document.rs", rev = "<commit-sha>", default-features = false, features = ["ir", "markdown"] }
plotter = { git = "https://github.com/notedge/document.rs", rev = "<commit-sha>" }
crawler = { git = "https://github.com/notedge/document.rs", rev = "<commit-sha>" }
template = { git = "https://github.com/notedge/document.rs", rev = "<commit-sha>" }
template-input-dejavu = { git = "https://github.com/notedge/document.rs", rev = "<commit-sha>" }
```

工具链见 [`rust-toolchain.toml`](rust-toolchain.toml)（nightly + rustfmt / clippy）。

## ✨ 能力概览

- **Document**：`DocumentGraph` IR、多格式 import/export、转换编排、排版与 SVG/PDF/HTML/光栅渲染；字体 **逻辑**在仓内，字体
  **字节**由 VuTeX 等注入。
- **Plotter**：Grammar of Graphics 与 Diagram/Chart **平行** Spec（不做万能 `plotter.ir`）；Mermaid / DOT / PlantUML / TikZ
  在 `plotter.input.*`。
- **Crawler**：seed → fetch → extract → store；抽取结果落到 `document.ir`；实现仓测试只留冒烟。
- **Template**：独立的顶层 `template.*` 命名空间，只做模板源 → 文本；输入引擎 `template.input.*`，构建框架合同 `template.framework`。

## 📊 和下游产品怎么分工

| 仓库                       | 角色                                  |
|----------------------------|---------------------------------------|
| **document.rs（本仓）**    | 共享 Rust 实现与合同                  |
| VuTeX / panduck / notedown | 编辑器、CLI、字体资产、视觉回归       |
| pandark 等采集产品         | NAPI / 调度 UI、大型 fixtures、合同套件 |

## 🧪 示例入口

完整可运行演示在下游产品仓。本仓以门面 crate 为准：

| 门面                                               | 说明                                       |
|----------------------------------------------------|--------------------------------------------|
| [`document`](projects/document/document/readme.md) | IR / formats / convert / layout / renderer |
| [`plotter`](projects/plotter/plotter/readme.md)    | GoG + diagram/chart + 语言前端             |
| [`crawler`](projects/crawler/crawler/readme.md)    | 抓取编排与再导出                           |

## 📜 License

[MPL-2.0](https://www.mozilla.org/en-US/MPL/2.0/)，仓内全文见 [`License.md`](License.md)。

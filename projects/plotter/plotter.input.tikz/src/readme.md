# plotter-input-tikz

TikZ 前端：`tikzpicture` 源直接降到 [`DisplayScene`](../plotter.scene)。

支持 `\draw` / `\fill` / `\filldraw`、`(x,y)` 坐标（裸数字按厘米，`cm`/`mm`/`pt` 后缀）、`--` 折线、`rectangle`、`circle`、路径内的 `node` 标签、独立的 `\node[options] at (x,y) {text}`（`above` / `below` / `left` / `right` 锚点与 `text=颜色`）、`dashed` / `thin` / `thick` / `very thick`、`line width`、`opacity` / `fill opacity`，以及 `xcolor` 基础色板（`black` / `white` / `red` / `green` / `blue` / `cyan` / `magenta` / `yellow` / `gray` / `darkgray` / `lightgray` / `brown` / `lime` / `olive` / `orange` / `pink` / `purple` / `teal` / `violet`）与 `scale`。`tikzpicture` 环境与裸体都接受。独立 `\node` 必须带 `at (x,y)` 位置。其余命令、选项、颜色与字符以 `davinci.diagram.unsupported` 加行内字节范围拒绝，不放宽也不近似。成功场景由宿主以 [`TIKZ_ALT`] 注册为 `data-diagram="TikZ picture"`。

产品树说明见 [`../readme.md`](../readme.md)。

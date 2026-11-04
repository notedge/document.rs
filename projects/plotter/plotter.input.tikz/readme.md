# plotter-input-tikz

TikZ 前端：`tikzpicture` 源直接降到 [`DisplayScene`](../plotter.scene)。

支持 `\draw` / `\fill` / `\filldraw`、`(x,y)` 坐标（裸数字按厘米，`cm`/`mm`/`pt` 后缀）、`--` 折线、`rectangle`、`circle`、`node` 标签、`dashed` / `thin` / `thick` / `very thick`、`line width`、`opacity` / `fill opacity`，以及 `black` / `red` / `blue` / `green` 与 `scale`。`tikzpicture` 环境与裸体都接受。其余命令、选项、颜色与字符以 `davinci.diagram.unsupported` 加行内字节范围拒绝，不放宽也不近似。成功场景由宿主以 [`TIKZ_ALT`] 注册为 `data-diagram="TikZ picture"`。

本 crate 是该能力的唯一实现位置，`VuTeX` 侧已删除本地 59 KB 实现改为调用 [`tikz_to_scene`]，因此任何 TikZ 语义变更都必须在这里做，不回下游补。

产品树说明见 [`../readme.md`](../readme.md)。

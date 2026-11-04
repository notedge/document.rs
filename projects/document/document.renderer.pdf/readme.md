# `document.renderer.pdf`

VuTeX 的 PDF 输出后端。

Diagram scene 以矢量几何绘制：图形保留自身的 fill 与 stroke，节点变换在根节点的一次 y 翻转下合成，标签按锚定对齐保持直立，
透明度经 `ExtGState` 资源生效——其 `/ca` 与 `/CA` 项在 `q` 与 `Q` 之间选择节点 alpha。

图片节点与页面级图片槽在其来源可解析时以栅格 XObject 嵌入。来源经调用方持有的 `PdfResources` 解析，优先级为：调用方提供的
字节载荷、带 base64 载荷的 `data:` URI、绝对文件系统路径、相对已配置 base directory 的路径，最后才是仅为向后兼容而保留的进程
工作目录。其余情况一律视为未解析，包括调用方未提供的 `http(s)` 或 `file:` URL。宿主若已自行拉取远程资产，可把字节交给
`PdfResources::with_supplied`，后端因而能嵌入它自己无法获取的图片；`PdfResources::with_base_dir` 让文档的相对引用跟随文档本身，
而不是进程目录。同一来源只生成一个对象，供所有引用该来源的槽复用。RGBA 或灰度 alpha PNG 会保留 alpha 通道而不是丢失它：
writer 把 alpha 平面输出为单通道 `/DeviceGray` 图像，在彩色图像字典中追加 `/SMask <n> 0 R` 项，并只把彩色图像注册为 `/ImN`
资源。8 位 `tRNS` 块带来的调色板透明度以同样方式展开，灰度或真彩 PNG 上的 colour-key `tRNS` 会变成遮罩，被键控的像素完全
透明。16 位样本在嵌入前收窄到高字节，Adam7 交错行在反过滤前先行去交错。每样本 1、2、4 位的 PNG 来源仍被拒绝。来源缺失、
未解析、不受支持或损坏时降级为虚线占位框，而不会让整篇文档失败。

SVG 来源转为矢量几何而非栅格，因此图标在任意输出分辨率下保持曲线，也不注册 `/XObject`。来源被判定为 SVG 的条件是：路径以
`.svg` 结尾，`data:` URI 声明 `image/svg+xml`，或调用方在 scene 资源引用中提供了该 MIME 类型。支持子集为：带 `width`、
`height` 与 `viewBox` 的 `<svg>`，`<g>` 的变换与透明度，`<path>`，带 `rx`/`ry` 的 `<rect>`，`<circle>`，`<ellipse>`，
`<line>`，`<polyline>`，`<polygon>`，路径命令 `M L H V C S Q T Z`（绝对与相对形式，含隐式重复），`fill`、`stroke`、
`stroke-width`、`fill-opacity`、`opacity`、`color`，作用于同一组属性的 `style` 声明，`none`，`currentColor`，`#rgb`，
`#rrggbb`，以及一张很小的命名颜色表。默认的 `xMidYMid meet` viewBox 适配会把图形居中并保持宽高比；缺少 `width` 或
`height` 时回退到对应的 `viewBox` 范围。同一来源只解析一次，每个槽绘制一次，在槽内适配并居中。

子集之外的一切都保留占位框，而不是画出错误内容：椭圆弧、渐变与图案、CSS class 与 `<style>` 规则、`<text>`、`<use>`、
symbol、mask、clip path、filter、marker、嵌套 `<svg>`、未知元素、畸形 XML，以及没有可用几何的路径。

writer 不需要任何第三方 crate。默认每个文本 run 都使用 `document_fonts` 正文字体的子集绘制，以 Identity-H `CIDFontType2`
程序嵌入，附 `FontFile2` 流与 `ToUnicode` 映射。文件中不出现任何 base-14 名称，因此阅读器或打印流水线没有可替换对象，
页面也不会重排：`Times-Roman` 与 `Courier` 已从打包输出中彻底消失，代码块同样如此。

字符来自一条回退链而非单一字体：汉字正文通常不带 emoji，emoji 字体通常不带汉字。每个程序覆盖此前程序未覆盖的字符，所以
混排文本从两个子集绘制，同时保持同一个文本矩阵，并在字形组之间切换字体。等宽 run 优先使用定宽字体，遇到汉字再回退到正文
字体，因为仓内不打包等宽汉字字体。请求的粗体或斜体样式先取该样式的字体，再回退到常规字体。

`PdfFontPolicy` 选择这条链。`Bundled` 为默认值，也是唯一能保证文件自包含的策略。`Discover` 探测系统 TrueType 字体
（先符号与拉丁字体，再 CJK 字体，最后单色 emoji，缺失或不可用的候选会被跳过），`File` 嵌入单个文件，`Files` 按编写顺序
嵌入列表。使用显式列表时，每一项都必须可读，某个程序未覆盖的字符会以 `vutex.render.pdf.glyph_missing` 错误报告并指名码位，
而不是丢弃字形或输出 `?`。`Base14Only` 把 WinAnsi 文本留在标准字体上，其余字符降级为 `?`，适合调用方明确要求文件不引用
任何字体程序的场景。

`render_pdf_with_resources` 同时接受字体策略与调用方的 `PdfResources` 上下文。

# `document.fonts`

Bundled font assets (Latin Modern Math, XITS Math, and the body faces Tinos,
Noto Serif SC, and Noto Sans Mono) with install helpers.

`BodyMetrics::bundled` reads the `hmtx` advance widths of every body face,
in em units, so line breaking reserves the width the PDF renderer paints.
`fallback_order` is the shared face selection both layout and the renderer
consult, and `FaceMetrics` keeps the legacy `kern` pairs a face may declare.

# `document.fonts`

Font bytes are supplied by the calling host: `document.fonts` ships no binary
asset. `BodyFaceSet` carries the body faces and `MathFontPack` the math pair.

`BodyMetrics::from_set` reads the `hmtx` advance widths of every supplied body face,
in em units, so line breaking reserves the width the PDF renderer paints.
`fallback_order` is the shared face selection both layout and the renderer
consult, and `FaceMetrics` keeps the legacy `kern` pairs a face may declare.

# `document.layout`

Page, block, line, table, and pagination layout.

## Math fonts

Formulas are typeset only when the host installs a math font
(`document_text::math::install_math_font`, for example Latin Modern Math).
Without one, `layout` keeps the author's source on the page as text and emits an
`Error` diagnostic, `vutex.layout.math.not_typeset`, with a span over the
formula. Both display `Math` blocks and inline math take that path, and the
layout is never reported as a clean pass.

With a font installed, formulas become outline paths. That is deliberate: the
glyphs are geometry, so the PDF text layer cannot extract or search a formula the
way it extracts prose. Prose stays extractable; math does not.

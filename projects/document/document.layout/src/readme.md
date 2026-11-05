# `document.layout`

Page, block, line, table, and pagination layout.

## Math fonts

Formulas are typeset only when the host installs a math font
(`document_text::math::install_math_font`, for example Latin Modern Math).
The repository ships no font bytes. `document-fonts` exposes a
one-shot, idempotent installer: call `document_fonts::install_math_fonts()`
once at startup with a `document_fonts::MathFontPack` the host owns
(Latin Modern Math and XITS Math). Take the bytes from that pack rather than
embedding another copy.
Installing fonts at the operating-system level is not enough: the resolver only
sees the in-process registry, so the host must load the font bytes itself.
Without one, `layout` keeps the author's source on the page as text and emits an
`Error` diagnostic, `vutex.layout.math.not_typeset`, with a span over the
formula. Both display `Math` blocks and inline math take that path, and the
layout is never reported as a clean pass.

The source-text fallback is drawn as ordinary text, so it stays searchable in
the PDF while showing no formula geometry. With host-installed font bytes the
glyphs are emitted as outlines instead, which is why a typeset formula is not
searchable in the PDF text layer. That is deliberate: prose stays extractable,
math does not.

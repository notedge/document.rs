# `document.fonts`

The single source of the font binaries that ship with this repository. Hosts
and tests must take the font bytes from here instead of embedding or copying a
second copy of the files.

## Math assets

| constant | font | license |
| --- | --- | --- |
| [`LATIN_MODERN_MATH`](crate::LATIN_MODERN_MATH) | `fonts/LatinModernMath-Regular.otf` | `fonts/LatinModernMath-Regular.GUST-FONT-LICENSE.md` |
| [`XITS_MATH`](crate::XITS_MATH) | `fonts/XITSMath-Regular.otf` | `fonts/XITSMath-Regular.OFL.md` |

The license texts are shipped with the binaries and must stay in the package:
Latin Modern Math is distributed under the GUST Font License (LaTeX Project
Public License 1.3c or later), XITS Math under the SIL Open Font License. They
use the `.md` extension because the repository ignores `*.txt`.

Math faces are never body faces. Both are outline-only math designs with no
regular text family, so a body run painted with them would look like a formula
and lose inter-word spacing.

## Body assets

| face | family | style | file | bytes | license |
| --- | --- | --- | --- | --- | --- |
| `Tinos-Regular.ttf` | Tinos | regular | Latin | 102,236 | `fonts/Tinos.OFL.md` |
| `Tinos-Bold.ttf` | Tinos | bold | Latin | 113,272 | `fonts/Tinos.OFL.md` |
| `Tinos-Italic.ttf` | Tinos | italic | Latin | 106,160 | `fonts/Tinos.OFL.md` |
| `NotoSerifSC-Regular.ttf` | Noto Serif SC | regular | Han | 3,094,248 | `fonts/NotoSerifSC.OFL.md` |
| `NotoSerifSC-Bold.ttf` | Noto Serif SC | bold | Han | 3,094,120 | `fonts/NotoSerifSC.OFL.md` |
| `NotoSansMono-Regular.ttf` | Noto Sans Mono | regular | Latin | 53,348 | `fonts/NotoSansMono.OFL.md` |

[`BODY_FACES`](crate::BODY_FACES) lists them in fallback order, and
`document.renderer.pdf` picks the face for a run from that list.

The Latin faces are Tinos because the shared layout pass measures text with a
Times-shaped advance model. A face wider than that model paints past the
advance layout reserved for it and collides with the next run, so the body face
has to stay on the Times metrics Tinos inherits; a designer-optimised serif such
as Noto Serif is 8–12 % wider and visibly overlaps. The Han faces are Noto Serif
SC, because a Latin face carries no Han glyph at all.

## Subsetting

The shipped files are not the upstream releases:

- The Latin faces are subset to ASCII, Latin-1, Latin Extended-A/B, general
  punctuation, currency signs, and the arithmetic relations a sentence may
  quote — 716 code points. The upstream Tinos file is 0.5–0.6 MB per face, the
  subset 0.1 MB.
- The Latin subsets keep the `kern` layout feature, so the `GPOS` `PairPos`
  format 1 pairs that carry inter-letter spacing survive as 113 / 117 / 112
  pairs for Regular / Bold / Italic. Everything else in `GPOS` and all of
  `GSUB` is dropped, so the faces carry no `liga` substitution. The pairs cost
  +652 / +668 / +660 bytes against the same subset without layout features.
- The Han faces are instances of the 25 MB `NotoSerifSC[wght].ttf` variable
  font at weights 400 and 700, then subset to GB 2312 (6,763 hanzi, all of
  0xA1–0xF7 lead rows), CJK punctuation, and full-width forms. A GB 2312
  document therefore cannot hit a missing glyph, but a rare character outside
  the set can: the renderer reports it as `vutex.render.pdf.glyph_missing`
  instead of drawing a blank.
- Every subset keeps the original copyright and licence strings in its `name`
  table and is renamed only to the upstream PostScript name, so the subset is
  identifiable in the PDF. No face declares a Reserved Font Name. The shipped
  binaries are modified (subsetted) versions redistributed under the SIL Open
  Font License, Version 1.1, whose text stays unmodified in `fonts/Tinos.OFL.md`.

Insets and outlines are untouched, so a subset glyph is byte-identical to its
upstream counterpart. Total asset weight is about 6.6 MB, of which the two Han
faces are 6.2 MB.

## Installing

[`install_default_math_font`](crate::install_default_math_font) registers both
bundled families with `document_text::math::install_math_font` on first call.
The install is guarded by a [`Once`], so calling it again — from several
startup paths, or from tests that share one process — is safe and a no-op.

`document.layout` resolves math through the in-process registry only, and the
preference order is Latin Modern Math before XITS Math. A host that never
installs these bytes keeps the authored source on the page as text and gets a
`vutex.layout.math.not_typeset` error diagnostic instead of typeset geometry.

The registry lives in the calling process. The fonts must be installed before
the first layout call; installing them afterwards does not retroactively
typeset an already laid-out page.

# `document.fonts`

The single source of the math fonts that ship with this repository. Hosts and
tests must take the font bytes from here instead of embedding or copying a
second copy of the binaries.

## Assets

| constant | font | license |
| --- | --- | --- |
| [`LATIN_MODERN_MATH`](crate::LATIN_MODERN_MATH) | `fonts/LatinModernMath-Regular.otf` | `fonts/LatinModernMath-Regular.GUST-FONT-LICENSE.md` |
| [`XITS_MATH`](crate::XITS_MATH) | `fonts/XITSMath-Regular.otf` | `fonts/XITSMath-Regular.OFL.md` |

The license texts are shipped with the binaries and must stay in the package:
Latin Modern Math is distributed under the GUST Font License (LaTeX Project
Public License 1.3c or later), XITS Math under the SIL Open Font License. They
use the `.md` extension because the repository ignores `*.txt`.

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

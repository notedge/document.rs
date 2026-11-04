# `document.input.tex`

TeX/LaTeX input profile. The syntax frontend stays outside VuTeX and the
document model owns the lowering, so this crate only selects the profile and
reports which constructs reached document semantics.

A construct the profile cannot map is recorded as an explicit conversion
status with a diagnostic code and a source span, never silently dropped or
replaced by an approximation.

`render_svg` runs the same import and then the shared layout, presentation, and
SVG render chain, so a host gets a rendered document and the layout diagnostics
from one call.

```rust,ignore
let rendered = document_input_tex::render_svg(
    "main.tex",
    "\\begin{document}\n\\section{Title}\nHello.\n\\end{document}\n",
)?;
println!("{}", rendered.svg);
```

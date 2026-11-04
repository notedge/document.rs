# `document.input.typst`

Typst input adapter for VuTeX. It selects the Notedown Typst lowering profile, projects
the resulting `document.ir` document graph into a `document.compose` conversion report, and
orchestrates the layout, presentation, and SVG render chain.

It owns no Typst parser and no document IR. Parsing stays in `oak-typst` and lowering
stays in `document.formats`, so Typst cannot grow a second style system or a second
typesetting kernel.

```rust,ignore
let rendered = document_input_typst::render_svg("main.typ", "= Title\n\nHello *world*.\n")?;
println!("{}", rendered.svg);
```

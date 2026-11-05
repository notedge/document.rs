# `document.input.formats`

Container document input profile. It probes bytes, selects the matching
`document.formats` import reader, returns the resulting
`document.ir::DocumentGraph`, and normalizes import loss into the shared
conversion report and diagnostics.

The crate owns no format parser, no package layout, and no document schema.
Probing only reads container headers and leading content signatures. Parsing
and semantic lowering stay in `document.formats`, so this profile cannot drift
from the format layer the other input adapters share.

`probe` reports what the bytes prove. A ZIP container is left ambiguous between
DOCX and EPUB until an extension hint resolves it, because the package parts
can only be separated by the reader. `import_bytes` imports a host-selected
format. `import_auto` imports only when the probe resolves exactly one format.
`import_file` reads one file and derives the hint from its extension.

A source above `MAX_SOURCE_BYTES` (8 MiB) is rejected before probing. An empty
or unrecognized source is refused instead of being defaulted to Markdown.
`Unsupported` loss and opaque unsupported blocks fail closed. Partial, lossy,
unresolved, and inferred loss stays in the report with `Warning` diagnostics.

Text languages are not imported here: Markdown, TeX, and Typst keep their own
profiles. The internal `notedown` semantic model is not an external input
format. Layout and rendering are not duplicated here either, so hosts continue
through `document.layout` and the renderer crates.

## Format matrix

`read` and `write` mirror the single capability table in
`document.formats::capabilities`. Roundtrip means structural text and block
coverage verified by this crate's tests, not layout fidelity.

| Format | read | write | roundtrip fidelity | lossy markers | refusal codes |
|---|---|---|---|---|---|
| `html` | Partial | Partial | structural, paragraph text | `import.html.partial_coverage` and the other `import.html.*` markers | `vutex.input.formats.import_failed` wrapping the reader error |
| `docx` | Partial | Partial | structural, paragraph text | `import.docx.partial_coverage` | `vutex.input.formats.import_failed` wrapping the reader error |
| `epub` | Partial | Partial | structural, paragraph text | `import.epub.partial_coverage` | `import.epub.unsupported_manifest_type` fails closed |
| `pdf` | Partial, text only | Partial, renderer chain | text content only | `import.pdf.*` markers, including `import.pdf.partial_coverage` | `vutex.input.formats.import_failed` wrapping the reader error |
| `doc` | Partial, text only | Unavailable | not promised, text-only read | `import.doc.partial_coverage`, `import.doc.complex_piece_table_unresolved` | `vutex.input.formats.import_failed` wrapping the reader error |

Routing refusals apply to every row:

| Condition | Code |
|---|---|
| no container header and no content signature | `vutex.input.formats.unknown_format` |
| ZIP package with no resolving hint | `vutex.input.formats.ambiguous_format` |
| probed text language outside this profile | `vutex.input.formats.unsupported_format` |
| declared format disagrees with the probe | `vutex.input.formats.format_mismatch` |
| source above the byte budget | `vutex.input.formats.source_too_large` |
| file cannot be read | `vutex.input.formats.file_read_failed` |
| graph violates a `notedown-ir` invariant | `vutex.input.formats.invalid_graph` |
| unsupported loss or unsupported opaque block | the original marker code, fail closed |

## Tests

`src/tests.rs` fixes the routing refusals and the roundtrips. Each roundtrip
builds a graph, exports through `document.formats`, imports it back through
this profile, and checks that the paragraph text survives:

| Case | Test |
|---|---|
| ZIP package stays ambiguous without a hint | `probe_keeps_a_zip_package_ambiguous_without_a_hint`, `auto_refuses_an_ambiguous_zip_package` |
| unknown text does not default to a format | `auto_refuses_unknown_text` |
| text language is refused by this profile | `auto_refuses_a_text_language_profile` |
| declared format must match the probe | `declared_format_must_match_the_probe` |
| HTML import keeps loss visible | `imports_html_and_keeps_loss_visible` |
| HTML roundtrip | `html_roundtrip_keeps_paragraph_text` |
| DOCX roundtrip | `docx_roundtrip_keeps_paragraph_text` |
| EPUB roundtrip | `epub_roundtrip_keeps_paragraph_text` |
| PDF text roundtrip | `pdf_roundtrip_keeps_text_content` |
| file routing and read failure | `file_import_routes_by_extension`, `file_import_reports_a_missing_file` |

# document-convert

Implementation modules for conversion routes and report assembly. Public API is `lib.rs`.

## Route availability

`supports_route(from, to)` is derived from the single capability table in
`document_formats::capabilities()`:

| capability status | routable |
| --- | --- |
| `Ready` | yes |
| `Partial` | yes — wired and usable; gaps surface per document as loss markers / diagnostics |
| `Planned` | no — not wired yet |
| `Unavailable` | no — explicit opt-out, including wired-but-not-deliverable exporters |
| missing entry | no — the matching cargo feature is off |

A route is available iff `from` is a wired reader (`READERS`), `to` is a wired
writer (`WRITERS`), the `Import` status of `from` is routable and the `Export`
status of `to` is routable. `READERS` / `WRITERS` describe wiring only; they no
longer decide availability by themselves.

`docx` and `pdf` exports are `Partial`, so `→ docx` and `→ pdf` routes stay
available. The conservative DOCX exporter refuses `Math` / `RawHtml` / opaque
blocks with an `unsupported` diagnostic instead of dropping them, and the PDF
writer runs `layout → presentation → renderer-pdf`, which keeps layout, embeds
glyphs for CJK text and typesets math.

`Partial` expresses "partially usable" without weakening the route: the route
stays `true` and each document reports its own gaps through
`coverage.loss` / `losses[].status` plus the diagnostic envelope. A route is
only withdrawn when the target cannot deliver for the general case at all.

`convert_bytes` applies the same predicate as an entry gate and returns
`unsupported_format` when a route is not available.

## Downstream consumers

`panduck-napi` exposes `supports_conversion` and `supported_routes` directly
from these two functions, so a `false` here becomes a `false` in the NAPI
surface with no extra mapping.

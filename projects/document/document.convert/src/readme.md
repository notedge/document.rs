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

Today `docx` and `pdf` exports are `Unavailable`, so every `→ docx` and
`→ pdf` route reports `false`: the conservative DOCX exporter rejects
`Math` / `RawHtml` / opaque blocks, and `export::pdf` cannot encode CJK and
prints math as source text. The writers stay compiled and exported; only the
advertised route is withdrawn.

`Partial` expresses "partially usable" without weakening the route: the route
stays `true` and each document reports its own gaps through
`coverage.loss` / `losses[].status` plus the diagnostic envelope. A route is
only withdrawn when the target cannot deliver for the general case at all.

`convert_bytes` applies the same predicate as an entry gate and returns
`unsupported_format` when a route is not available.

## Downstream consumers

`panduck-napi` exposes `supports_conversion` and `supported_routes` directly
from these two functions, so a `false` here becomes a `false` in the NAPI
surface with no extra mapping. PDF consumers in panduck must use the
`import → layout → presentation → renderer-pdf` chain instead of the withdrawn
`convert → pdf` writer; see the migration checklist in the route honesty
report.

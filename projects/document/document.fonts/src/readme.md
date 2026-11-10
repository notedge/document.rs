# document.fonts

Body text metrics and math install hooks. This crate ships **no font bytes**:
the host owns the asset pack and hands the bytes in.

## Body faces

A host builds a [`BodyFaceSet`] either from bytes it embedded

```text
let set = BodyFaceSet::new(vec![
    BodyFace::new(BodyFamily::Serif, BodyScript::Latin, BodyFaceStyle::Regular, TINOS_REGULAR),
    BodyFace::new(BodyFamily::Named("Template Serif"), BodyScript::Latin, BodyFaceStyle::Regular, TEMPLATE_BYTES),
]).expect("every face carries a distinct triple");
```

or from a directory laid out per [`FACE_FILES`]:

```text
let set = BodyFaceSet::load_from(Path::new("fonts"))?;
```

`BodyFaceSet::new` rejects a repeated `(family, script, style)` triple, and
[`BodyFamily::Named`] carries a host-declared family whose name is compared as
a full case-sensitive string. `BodyFaceSet::family_named` and
`BodyMetrics::key_for_name` resolve such a name exactly and return `None` for
an unknown one; the name never folds onto `Serif`.

`Tinos-Regular.ttf`, `Tinos-Bold.ttf`, `Tinos-Italic.ttf`, `NotoSerifSC-Regular.ttf`,
`NotoSerifSC-Bold.ttf`, and `NotoSansMono-Regular.ttf` are the names a directory pack
is expected to carry. The Latin faces share Times New Roman's advance widths, which keeps
the width model `document_layout` uses to break lines. The CJK faces are Noto Serif SC.

[`BodyMetrics::from_set`] reads `hmtx` and `cmap` from the supplied faces in em units, so
layout reserves exactly what the renderer paints. A face that cannot be parsed is reported
by name in [`BodyMetrics::unreadable`] instead of silently falling back to a heuristic.

## Math faces

[`install_math_fonts`] registers a [`MathFontPack`] (Latin Modern Math and XITS Math)
with `document_text::math`. The pack carries `'static` slices because the math registry
borrows them for the rest of the process, so a host passes `include_bytes!` payloads.

## Where the bytes live

Font binaries and their licences live in the consuming host, not in this crate. The
VuTeX asset pack under `projects/vutex.rs/fonts/` carries Tinos, Noto Sans Mono,
Noto Serif SC, Latin Modern Math, XITS Math, and the matching OFL / GUST licence texts.

#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("readme.md")]

use std::{
    fmt, io,
    path::Path,
    sync::{Arc, Once},
};

use document_text::math::{MathFontFamily, install_math_font};

mod metrics;

pub use metrics::{BodyFaceKey, BodyMetrics, FaceMetrics, MetricsError, body_style, fallback_order, key_for_run};

/// Family of one body face.
///
/// Body prose and code blocks need different faces for different reasons: prose
/// wants a reading face whose weight carries structure, while code needs a fixed
/// advance so columns line up. The two never substitute for each other, except
/// that a code block containing Han characters falls back to the body CJK face
/// because no monospaced CJK face is required.
///
/// [`Named`](Self::Named) carries any further family a host registers, for
/// example the family a template declares as its body text face. The name is
/// `'static` because the host interns it at registration, comparison is exact
/// and case-sensitive, and no code may fold a name onto [`Serif`](Self::Serif)
/// by substring guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodyFamily {
    /// Upright reading face for prose.
    Serif,
    /// Fixed-advance face for code and verbatim text.
    Mono,
    /// A family registered by the host under its own name.
    Named(&'static str),
}

/// Script coverage of one body face.
///
/// Latin and CJK are separate assets on purpose: a CJK face carries tens of
/// thousands of outlines and would bury a Latin document under megabytes of
/// unused glyphs, while a Latin face has no Han coverage at all. A renderer
/// builds a fallback chain from both, so a mixed line paints from two programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyScript {
    /// Latin, Greek-free Western text: ASCII, Latin-1, Latin Extended-A/B,
    /// general punctuation, and the common mathematical relation signs a body
    /// sentence may quote.
    Latin,
    /// Simplified Chinese body text: GB 2312 hanzi, CJK punctuation, and
    /// full-width forms.
    Cjk,
}

/// Style of one body face.
///
/// Only the styles a body paragraph actually asks for are required. There is no
/// bold-italic face and no CJK italic face, because the document IR has no
/// bold-italic style and a slanted Han face is not part of the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BodyFaceStyle {
    /// Upright regular weight.
    Regular,
    /// Bold weight.
    Bold,
    /// Slanted regular weight, Latin only.
    Italic,
}

/// One body face supplied by the calling host.
#[derive(Debug, Clone)]
pub struct BodyFace {
    /// Family this face belongs to.
    pub family: BodyFamily,
    /// Script this face covers.
    pub script: BodyScript,
    /// Style this face carries.
    pub style: BodyFaceStyle,
    /// Complete TrueType file, embedded through a per-document subset.
    pub bytes: Arc<[u8]>,
}

impl BodyFace {
    /// Build one face from bytes the host owns.
    pub fn new(family: BodyFamily, script: BodyScript, style: BodyFaceStyle, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self { family, script, style, bytes: bytes.into() }
    }
}

/// File name, family, script, and style of one face a directory pack must carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceSpec {
    /// File name inside the pack directory.
    pub file_name: &'static str,
    /// Family the file serves.
    pub family: BodyFamily,
    /// Script the file covers.
    pub script: BodyScript,
    /// Style the file carries.
    pub style: BodyFaceStyle,
}

/// Faces a body pack is expected to carry, in fallback order.
///
/// The order groups by family, then Latin before CJK, so a Latin character
/// always comes from the Latin face even when the CJK face happens to carry it
/// too. A requested style is served by the faces of that style first, then by
/// the regular faces, which is why the list repeats script order per style.
pub const FACE_FILES: &[FaceSpec] = &[
    FaceSpec {
        file_name: "Tinos-Regular.ttf",
        family: BodyFamily::Serif,
        script: BodyScript::Latin,
        style: BodyFaceStyle::Regular,
    },
    FaceSpec { file_name: "Tinos-Bold.ttf", family: BodyFamily::Serif, script: BodyScript::Latin, style: BodyFaceStyle::Bold },
    FaceSpec {
        file_name: "Tinos-Italic.ttf",
        family: BodyFamily::Serif,
        script: BodyScript::Latin,
        style: BodyFaceStyle::Italic,
    },
    FaceSpec {
        file_name: "NotoSerifSC-Regular.ttf",
        family: BodyFamily::Serif,
        script: BodyScript::Cjk,
        style: BodyFaceStyle::Regular,
    },
    FaceSpec {
        file_name: "NotoSerifSC-Bold.ttf",
        family: BodyFamily::Serif,
        script: BodyScript::Cjk,
        style: BodyFaceStyle::Bold,
    },
    FaceSpec {
        file_name: "NotoSansMono-Regular.ttf",
        family: BodyFamily::Mono,
        script: BodyScript::Latin,
        style: BodyFaceStyle::Regular,
    },
];

/// A face pack could not be read from its directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceSetError {
    file: String,
    reason: String,
}

impl fmt::Display for FaceSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot load body face `{}`: {}", self.file, self.reason)
    }
}

impl std::error::Error for FaceSetError {}

/// Body faces supplied by one host, in fallback order.
///
/// `document.fonts` never ships font bytes. A host either embeds its own asset
/// pack and calls [`BodyFaceSet::new`], or keeps the files in a directory laid
/// out as [`FACE_FILES`] describes and calls [`BodyFaceSet::load_from`].
///
/// A pack may carry [`Named`](BodyFamily::Named) families beside the built-in
/// ones. One named family can hold several faces (Latin and CJK scripts,
/// regular, bold, and italic styles), and the Latin face is declared before the
/// CJK face so coverage keeps the same priority [`FACE_FILES`] describes.
#[derive(Debug, Clone, Default)]
pub struct BodyFaceSet {
    faces: Vec<BodyFace>,
}

impl BodyFaceSet {
    /// Build a pack from faces the host already holds.
    ///
    /// Repeated registration of one `(family, script, style)` triple is
    /// rejected instead of letting face order decide which face serves a run.
    pub fn new(faces: Vec<BodyFace>) -> Result<Self, FaceSetError> {
        let mut seen = Vec::with_capacity(faces.len());
        for face in &faces {
            let triple = (face.family, face.script, face.style);
            if seen.contains(&triple) {
                return Err(FaceSetError {
                    file: face_triple_label(triple),
                    reason: "family, script, and style are already registered by an earlier face".to_string(),
                });
            }
            seen.push(triple);
        }
        Ok(Self { faces })
    }

    /// Load a pack from a directory that carries one file per [`FACE_FILES`] entry.
    ///
    /// Every file is read eagerly, because a missing face would only surface as
    /// a missing glyph on a page much later. A read failure names the file.
    pub fn load_from(dir: &Path) -> Result<Self, FaceSetError> {
        let mut faces = Vec::with_capacity(FACE_FILES.len());
        for spec in FACE_FILES {
            let path = dir.join(spec.file_name);
            let bytes = std::fs::read(&path)
                .map_err(|error| FaceSetError { file: path.display().to_string(), reason: io_reason(&error) })?;
            faces.push(BodyFace::new(spec.family, spec.script, spec.style, bytes));
        }
        Ok(Self { faces })
    }

    /// Faces in fallback order.
    pub fn faces(&self) -> &[BodyFace] {
        &self.faces
    }

    /// The registered [`Named`](BodyFamily::Named) family whose name equals
    /// `name`, compared as a full case-sensitive string.
    ///
    /// An unknown name returns `None`; the lookup never folds onto
    /// [`Serif`](BodyFamily::Serif), because a missing family must stay a
    /// distinct failure from a face that lacks one glyph.
    pub fn family_named(&self, name: &str) -> Option<BodyFamily> {
        self.faces
            .iter()
            .find(|face| matches!(face.family, BodyFamily::Named(registered) if registered == name))
            .map(|face| face.family)
    }

    /// Number of faces in the pack.
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// Whether the pack carries no face at all.
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}

/// Human readable label of one `(family, script, style)` registration triple.
fn face_triple_label(triple: (BodyFamily, BodyScript, BodyFaceStyle)) -> String {
    let (family, script, style) = triple;
    let name = match family {
        BodyFamily::Named(name) => name.to_string(),
        other => format!("{other:?}"),
    };
    format!("{name}/{script:?}/{style:?}")
}

/// Human readable cause of an unreadable face directory entry.
fn io_reason(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "file not found".to_string(),
        _ => error.to_string(),
    }
}

/// Math faces a host installs into `document_text`.
///
/// Both payloads are `'static` because the math registry keeps a borrowed
/// pointer for the whole process, so a host passes the bytes it embedded with
/// `include_bytes!` rather than a buffer it read at startup.
#[derive(Debug, Clone, Copy)]
pub struct MathFontPack {
    /// Latin Modern Math (Computer Modern OpenType successor), GUST Font License.
    pub latin_modern: &'static [u8],
    /// XITS Math (Times-like, KaTeX lineage), SIL Open Font License.
    pub xits: &'static [u8],
}

impl MathFontPack {
    /// Pair two math payloads for one install.
    pub fn new(latin_modern: &'static [u8], xits: &'static [u8]) -> Self {
        Self { latin_modern, xits }
    }

    /// Read both payloads from a directory that carries `LatinModernMath-Regular.otf`
    /// and `XITSMath-Regular.otf`.
    ///
    /// The bytes are leaked because the math registry borrows them for the whole
    /// process, so a host that keeps its pack on disk pays the copy once.
    pub fn load_from(dir: &Path) -> Result<Self, FaceSetError> {
        Ok(Self {
            latin_modern: read_static(&dir.join("LatinModernMath-Regular.otf"))?,
            xits: read_static(&dir.join("XITSMath-Regular.otf"))?,
        })
    }
}

/// Read one file into a slice that lives as long as the process.
fn read_static(path: &Path) -> Result<&'static [u8], FaceSetError> {
    let bytes =
        std::fs::read(path).map_err(|error| FaceSetError { file: path.display().to_string(), reason: io_reason(&error) })?;
    Ok(Box::leak(bytes.into_boxed_slice()))
}

static INSTALL_MATH: Once = Once::new();

/// Install the host-supplied math faces as the process-wide default.
///
/// Registers the Latin Modern bytes for [`MathFontFamily::LmMath`] and the XITS
/// bytes for [`MathFontFamily::XitsMath`] through
/// `document_text::math::install_math_font`. The install runs once per process,
/// so the first pack wins and repeated calls are a no-op.
pub fn install_math_fonts(pack: &MathFontPack) {
    INSTALL_MATH.call_once(|| {
        install_math_font(MathFontFamily::LmMath, pack.latin_modern);
        install_math_font(MathFontFamily::XitsMath, pack.xits);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_from_reads_every_declared_file() {
        let dir = std::env::temp_dir().join(format!("document-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        for spec in FACE_FILES {
            std::fs::write(dir.join(spec.file_name), b"placeholder").expect("write face");
        }
        let set = BodyFaceSet::load_from(&dir).expect("pack loads");
        assert_eq!(set.len(), FACE_FILES.len());
        assert_eq!(&*set.faces()[0].bytes, b"placeholder");
        for spec in FACE_FILES {
            let _ = std::fs::remove_file(dir.join(spec.file_name));
        }
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn load_from_names_the_missing_file() {
        let dir = std::env::temp_dir().join(format!("document-fonts-missing-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let error = BodyFaceSet::load_from(&dir).expect_err("an empty directory has no face");
        assert!(error.to_string().contains("Tinos-Regular.ttf"), "{error}");
        let _ = std::fs::remove_dir(&dir);
    }
}

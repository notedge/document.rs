//! 限位与算子墨迹的几何回归。
//!
//! 只比较整条公式外接框的回归拦不住「限位压进算子笔画」：整框高度与宽度可以
//! 在限位缩进算子内部后仍然保持不变。这里逐 glyph 求绝对基线，按 `GlyphMetrics`
//! 的墨迹高度与深度展开，并用字形轮廓把算子的墨迹裁剪到限位所占的水平带内，
//! 再断言该带内的上下间隙不小于半像素。
//!
//! 对 `\int_0^1` 这类侧排下标，参考引擎本身就把下标收在积分号的墨迹外接框
//! 之内（积分号的下摆向左弯，下标落在它的右下方），所以整框求交会把正确的
//! TeX 位置判成错误。水平带裁剪保留「限位不得压进笔画」的原意，同时不要求
//! 参考引擎做不到的整框分离。

mod dispatch;

use std::path::{Path, PathBuf};

use document_fonts::{MathFontPack, install_math_fonts};
use document_layout::math::{LayoutNode, layout_formula_source_for};
use document_text::math::MathFontFamily;

/// Math bytes are host-owned, so read the VuTeX pack beside this repository,
/// or the directory named by `VUTEX_FONT_DIR`. A missing pack fails the test.
fn host_font_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VUTEX_FONT_DIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("workspace root")
        .join("VuTeX")
        .join("projects")
        .join("vutex.rs")
        .join("fonts")
}

pub(crate) fn install_host_math_fonts() {
    let dir = host_font_dir();
    assert!(dir.is_dir(), "math regression needs the host font pack at {}; set VUTEX_FONT_DIR", dir.display());
    install_math_fonts(&MathFontPack::load_from(&dir).expect("math font pack"));
}

/// 半像素容差。
const TOLERANCE: f64 = 0.5;

/// 画布坐标下的墨迹外接框，`top` 小于 `bottom`（y 向下）。
#[derive(Clone, Copy, Debug)]
struct InkBox {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

#[derive(Clone, Debug)]
struct PlacedGlyph {
    char: Option<char>,
    size: f64,
    ink: InkBox,
    /// 字形原点与基线，用于把以 em 为单位、y 向上的轮廓换算到画布。
    origin_x: f64,
    baseline_y: f64,
    /// 字形轮廓，y 向上、以 em 为单位。
    outline: Option<String>,
}

/// 按 [`lower_layout_to_paint`] 的规则走盒树：HBox 的 shift 走 Y 轴、VBox 的
/// shift 走 X 轴，`GlyphMetrics` 的 height 与 depth 是墨迹上下界。
fn collect(node: &LayoutNode, x: f64, y: f64, out: &mut Vec<PlacedGlyph>) {
    match node {
        LayoutNode::HBox(hbox) => {
            let mut cursor = 0.0;
            let baseline = y + hbox.shift.to_f64();
            for child in &hbox.children {
                collect(child, x + cursor, baseline, out);
                cursor += child.width().to_f64();
            }
        }
        LayoutNode::VBox(vbox) => {
            let mut cursor = -vbox.height.to_f64();
            let left = x + vbox.shift.to_f64();
            for child in &vbox.children {
                collect(child, left, y + cursor + child.height().to_f64(), out);
                cursor += child.height().to_f64() + child.depth().to_f64();
            }
        }
        LayoutNode::Glyph(glyph) => {
            let metrics = &glyph.metrics;
            out.push(PlacedGlyph {
                char: glyph.char,
                size: glyph.size.to_f64(),
                ink: InkBox {
                    left: x,
                    right: x + metrics.width.to_f64(),
                    top: y - metrics.height.to_f64(),
                    bottom: y + metrics.depth.to_f64(),
                },
                origin_x: x,
                baseline_y: y,
                outline: glyph.path_data.clone(),
            });
        }
        _ => {}
    }
}

/// 字形轮廓在画布坐标下的点，轮廓自己的 y 轴向上，画布 y 轴向下。
fn outline_points(glyph: &PlacedGlyph) -> Vec<(f64, f64)> {
    let Some(d) = glyph.outline.as_deref()
    else {
        return Vec::new();
    };
    let mut points = Vec::new();
    let mut command = 'M';
    let mut numbers = Vec::new();
    fn flush(glyph: &PlacedGlyph, command: char, numbers: &mut Vec<f64>, points: &mut Vec<(f64, f64)>) {
        let arity = match command {
            'C' => 6,
            'Q' => 4,
            'Z' => 0,
            _ => 2,
        };
        if arity == 0 {
            numbers.clear();
            return;
        }
        for chunk in numbers.chunks_exact(arity) {
            let px = chunk[arity - 2];
            let py = chunk[arity - 1];
            points.push((glyph.origin_x + px * glyph.size, glyph.baseline_y - py * glyph.size));
        }
        numbers.clear();
    }
    for token in d.split([' ', ',']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        match token.chars().next().expect("non-empty") {
            first if first.is_ascii_alphabetic() => {
                flush(glyph, command, &mut numbers, &mut points);
                command = first;
            }
            _ => {
                if let Ok(value) = token.parse::<f64>() {
                    numbers.push(value);
                }
            }
        }
    }
    flush(glyph, command, &mut numbers, &mut points);
    points
}

/// 算子墨迹在 `left..=right` 水平带内的纵向范围。
fn operator_band(operator: &PlacedGlyph, left: f64, right: f64) -> Option<(f64, f64)> {
    let mut band: Option<(f64, f64)> = None;
    for (x, y) in outline_points(operator) {
        if x < left - TOLERANCE || x > right + TOLERANCE {
            continue;
        }
        band = Some(match band {
            Some((top, bottom)) => (top.min(y), bottom.max(y)),
            None => (y, y),
        });
    }
    band
}

fn glyphs(tex: &str, display: bool) -> Vec<PlacedGlyph> {
    install_host_math_fonts();
    let (_, root, _) = layout_formula_source_for(tex, display, 10.0, MathFontFamily::LmMath).expect("layout succeeds");
    let mut placed = Vec::new();
    collect(&root, 0.0, 0.0, &mut placed);
    placed
}

fn split_operator(placed: &[PlacedGlyph]) -> (usize, Vec<usize>) {
    let index = placed
        .iter()
        .enumerate()
        .filter(|(_, glyph)| matches!(glyph.char, Some('∫' | '∑')))
        .max_by(|(_, a), (_, b)| a.size.partial_cmp(&b.size).expect("finite size"))
        .map(|(index, _)| index)
        .expect("formula has a large operator");
    let scripts = placed.iter().enumerate().filter(|(i, _)| *i != index).map(|(index, _)| index).collect();
    (index, scripts)
}

/// 返回 `(算子, 脚本, 带内算子上沿, 带内算子下沿)`，没有交点时为 `None` 的带。
fn band_clearance(label: &str, tex: &str, display: bool) {
    let placed = glyphs(tex, display);
    let (operator_index, scripts) = split_operator(&placed);
    let operator = &placed[operator_index];
    assert!(!scripts.is_empty(), "{label}: scripts missing");
    for script_index in scripts {
        let script = &placed[script_index];
        let band = operator_band(operator, script.ink.left, script.ink.right);
        let Some((band_top, band_bottom)) = band
        else {
            continue;
        };
        let above_clearance = band_top - script.ink.bottom;
        let below_clearance = script.ink.top - band_bottom;
        assert!(
            above_clearance >= TOLERANCE || below_clearance >= TOLERANCE,
            "{label}: script {:?} {:?} overlaps operator ink in x {}..{}, band {}..{}, clearances {} / {}",
            script.char,
            script.ink,
            script.ink.left,
            script.ink.right,
            band_top,
            band_bottom,
            above_clearance,
            below_clearance
        );
    }
}

#[test]
fn side_scripts_clear_the_operator_ink() {
    band_clearance("display int_0^1", "\\int_0^1", true);
    band_clearance("display int_-inf^inf", "\\int_{-\\infty}^{\\infty}", true);
    band_clearance("inline int_-inf^inf", "\\int_{-\\infty}^{\\infty}", false);
    band_clearance("inline sum_i^n", "\\sum_{i=1}^{n}", false);
}

#[test]
fn side_script_offsets_match_the_reference() {
    // 参考引擎（Tectonic 0.17 + Latin Modern Math，10pt、4x 栅格）实测：
    // 上标锚点落在算子的 advance 上，下标锚点落在 `advance - italic correction`，
    // 纵向分别是 display 1.1817/0.9971 em、inline 0.6277/0.4442 em。
    // 旧布局把两个脚本都收在算子 advance 左侧 0.27 em 内，这条用例直接拦死。
    for (label, tex, display, sup, sub) in [
        ("display int_0^1", "\\int_0^1", true, (9.99, -11.86), (4.08, 10.01)),
        ("inline int_0^1", "\\int_0^1", false, (6.65, -6.30), (3.33, 4.46)),
    ] {
        let placed = glyphs(tex, display);
        let (_, scripts) = split_operator(&placed);
        for script_index in scripts {
            let script = &placed[script_index];
            let expected = match script.char {
                Some('1') => sup,
                Some('0') => sub,
                _ => continue,
            };
            let dx = (script.origin_x - expected.0).abs();
            let dy = (script.baseline_y - expected.1).abs();
            assert!(
                dx <= TOLERANCE && dy <= TOLERANCE,
                "{label}: script {:?} sits at ({}, {}), reference ({}, {})",
                script.char,
                script.origin_x,
                script.baseline_y,
                expected.0,
                expected.1
            );
        }
    }
}

/// `\int_{-\infty}^{\infty} e^{-x^2}\,\mathrm{d}x=\sqrt{\pi}` 的逐锚点对照。
///
/// 参考值取自 Tectonic 0.17 + Latin Modern Math（9.963 pt 排版，按 10 pt
/// 换算）。按绘制顺序逐 glyph 比对原点，容差半像素。整框 `diffRatio` 会被
/// 参考的 0.37% 字号差放大，所以这里只用锚点。
#[test]
fn user_expression_anchors_match_the_reference() {
    // (origin_x, baseline_y)，顺序为 ∫ ∞ − ∞ e − x 2 d x = √ π。
    const DISPLAY_ANCHORS: [(f64, f64); 13] = [
        (0.000, 0.000),
        (9.990, -11.860),
        (4.080, 10.010),
        (9.526, 10.010),
        (20.129, 0.000),
        (24.789, -3.630),
        (30.235, -3.630),
        (34.771, -6.171),
        (40.842, 0.000),
        (46.402, 0.000),
        (54.892, 0.000),
        (65.457, -7.639),
        (73.788, 0.000),
    ];
    const INLINE_ANCHORS: [(f64, f64); 13] = [
        (0.000, 0.000),
        (6.651, -6.300),
        (3.330, 4.460),
        (8.776, 4.460),
        (18.916, 0.000),
        (23.575, -3.630),
        (29.021, -3.630),
        (33.557, -6.171),
        (39.629, 0.000),
        (45.188, 0.000),
        (53.678, 0.000),
        (64.244, -7.150),
        (72.575, 0.000),
    ];
    let tex = "\\int_{-\\infty}^{\\infty} e^{-x^2}\\,\\mathrm{d}x=\\sqrt{\\pi}";
    for (label, display, expected) in [("display", true, DISPLAY_ANCHORS), ("inline", false, INLINE_ANCHORS)] {
        let placed = glyphs(tex, display);
        assert_eq!(placed.len(), expected.len(), "{label}: glyph count");
        for (index, (glyph, (x, y))) in placed.iter().zip(expected.iter()).enumerate() {
            assert!(
                (glyph.origin_x - x).abs() <= TOLERANCE && (glyph.baseline_y - y).abs() <= TOLERANCE,
                "{label}: glyph {index} {:?} sits at ({:.3}, {:.3}), reference ({:.3}, {:.3})",
                glyph.char,
                glyph.origin_x,
                glyph.baseline_y,
                x,
                y
            );
        }
    }
}

#[test]
fn display_limits_keep_a_vertical_gap() {
    for tex in ["\\sum_{i=1}^{n}", "\\sum_{-\\infty}^{\\infty}"] {
        let placed = glyphs(tex, true);
        let (operator_index, scripts) = split_operator(&placed);
        let operator = &placed[operator_index];
        let mut above = 0;
        let mut below = 0;
        for script_index in scripts {
            let script = &placed[script_index];
            let Some((band_top, band_bottom)) = operator_band(operator, script.ink.left, script.ink.right)
            else {
                continue;
            };
            if script.ink.bottom <= band_top {
                above += 1;
                let gap = band_top - script.ink.bottom;
                assert!(gap >= TOLERANCE, "{tex}: upper limit {:?} touches the operator, gap {gap}", script.char);
            }
            else if script.ink.top >= band_bottom {
                below += 1;
                let gap = script.ink.top - band_bottom;
                assert!(gap >= TOLERANCE, "{tex}: lower limit {:?} touches the operator, gap {gap}", script.char);
            }
            else {
                panic!("{tex}: limit {:?} overlaps the operator band", script.char);
            }
        }
        assert!(above >= 1 && below >= 1, "{tex}: limits are not stacked above and below");
    }
}

/// 合同 28 验收 2：同一 MathIR 输入，`typst` 标签路径与 TeX 路径的逐 glyph
/// 几何与字形轮廓完全一致。两条路径只差标签与 `source` 文本，引擎消费的是
/// 同一棵共享 MathIR，所以任何分歧都意味着派发路径偷偷做了语法嗅探。
#[test]
fn typst_and_tex_tags_lay_the_same_math_ir_identically() {
    use document_ir::{MathAtom, MathClass, MathDisplay, MathExpression, MathFormula};
    use document_layout::math::layout_formula_for;

    install_host_math_fonts();
    let expression = MathExpression::Sequence(vec![
        MathExpression::Script {
            base: Box::new(MathExpression::Atom(MathAtom { text: "x".into(), class: MathClass::Identifier, span: None })),
            superscript: Some(Box::new(MathExpression::Number { text: "2".into(), span: None })),
            subscript: None,
        },
        MathExpression::Atom(MathAtom { text: "+".into(), class: MathClass::Binary, span: None }),
        MathExpression::Number { text: "1".into(), span: None },
    ]);
    let typst = MathFormula {
        source: "x^2 + 1".into(),
        language: Some("typst".into()),
        display: MathDisplay::Inline,
        expression: expression.clone(),
    };
    let tex = MathFormula { source: "x^2+1".into(), language: Some("latex".into()), display: MathDisplay::Inline, expression };

    let (_, typst_root, _) = layout_formula_for(&typst, 10.0, MathFontFamily::LmMath).expect("typst path lays out");
    let (_, tex_root, _) = layout_formula_for(&tex, 10.0, MathFontFamily::LmMath).expect("tex path lays out");
    let mut typst_placed = Vec::new();
    collect(&typst_root, 0.0, 0.0, &mut typst_placed);
    let mut tex_placed = Vec::new();
    collect(&tex_root, 0.0, 0.0, &mut tex_placed);

    assert_eq!(typst_placed.len(), tex_placed.len(), "the two paths place the same number of glyphs");
    for (index, (from_typst, from_tex)) in typst_placed.iter().zip(tex_placed.iter()).enumerate() {
        assert_eq!(from_typst.char, from_tex.char, "glyph {index} must be the same character");
        assert_eq!(from_typst.outline, from_tex.outline, "glyph {index} must carry the same path data");
        assert!(
            (from_typst.origin_x - from_tex.origin_x).abs() <= TOLERANCE
                && (from_typst.baseline_y - from_tex.baseline_y).abs() <= TOLERANCE,
            "glyph {index} {:?} sits at ({}, {}), the tex path ({}, {})",
            from_typst.char,
            from_typst.origin_x,
            from_typst.baseline_y,
            from_tex.origin_x,
            from_tex.baseline_y
        );
    }
}

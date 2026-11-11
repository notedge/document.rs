//! Document 门面 — 只做稳定再导出。
//!
//! 每个模块是通向某个 `document-*` crate 的薄窗口，本 crate 不定义业务逻辑。

#![deny(missing_docs)]

/// 文档语义 IR（`DocumentGraph`）。
#[cfg(feature = "ir")]
pub mod ir {
    pub use document_ir::*;
}

/// 格式 import / export。
#[cfg(feature = "formats")]
pub mod formats {
    pub use document_formats::*;
}

/// 转换合同与 I/O 辅助。
#[cfg(feature = "types")]
pub mod types {
    pub use document_types::*;
}

/// 转换域诊断。
#[cfg(feature = "diagnostic")]
pub mod diagnostic {
    pub use document_diagnostic::*;
}

/// 转换编排。
#[cfg(feature = "convert")]
pub mod convert {
    pub use document_convert::*;
}

/// 排版侧共享 ID / 诊断 / 报告。
#[cfg(feature = "compose")]
pub mod compose {
    pub use document_compose::*;
}

/// 语义样式与主题。
#[cfg(feature = "style")]
pub mod style {
    pub use document_style::*;
}

/// 字体处理与 shaping（不含字体字节）。
#[cfg(feature = "text")]
pub mod text {
    pub use document_text::*;
}

/// 展示 / 流式 presentation IR。
#[cfg(feature = "presentation")]
pub mod presentation {
    pub use document_presentation::*;
}

/// 分页与块布局。
#[cfg(feature = "layout")]
pub mod layout {
    pub use document_layout::*;
}

/// 输出后端合同。
#[cfg(feature = "renderer")]
pub mod renderer {
    pub use document_renderer::*;

    /// SVG 后端。
    #[cfg(feature = "renderer-svg")]
    pub mod svg {
        pub use document_renderer_svg::*;
    }

    /// PDF 后端。
    #[cfg(feature = "renderer-pdf")]
    pub mod pdf {
        pub use document_renderer_pdf::*;
    }

    /// HTML 后端。
    #[cfg(feature = "renderer-html")]
    pub mod html {
        pub use document_renderer_html::*;
    }

    /// 光栅后端。
    #[cfg(feature = "renderer-raster")]
    pub mod raster {
        pub use document_renderer_raster::*;
    }
}

/// 输入适配器。
pub mod input {
    /// Markdown → IR / 排版闭环。
    #[cfg(feature = "input-markdown")]
    pub mod markdown {
        pub use document_input_markdown::*;
    }

    /// TeX / LaTeX 输入。
    #[cfg(feature = "input-tex")]
    pub mod tex {
        pub use document_input_tex::*;
    }

    /// Typst 输入。
    #[cfg(feature = "input-typst")]
    pub mod typst {
        pub use document_input_typst::*;
    }

    /// 容器文档输入（HTML / DOCX / EPUB / PDF / DOC）。
    #[cfg(feature = "input-formats")]
    pub mod formats {
        pub use document_input_formats::*;
    }
}

/// 模板引擎合同。
#[cfg(feature = "template")]
pub mod template {
    pub use document_template::*;

    /// 模板语言前端。
    pub mod input {
        /// Handlebars 语法子集。
        #[cfg(feature = "template-input-hbs")]
        pub mod hbs {
            pub use document_template_input_hbs::*;
        }

        /// Jinja2 语法子集。
        #[cfg(feature = "template-input-jinja")]
        pub mod jinja {
            pub use document_template_input_jinja::*;
        }

        /// Dejavu 模板语言。
        #[cfg(feature = "template-input-dejavu")]
        pub mod dejavu {
            pub use document_template_input_dejavu::*;
        }
    }

    /// 构建框架合同。
    #[cfg(feature = "template-framework")]
    pub mod framework {
        pub use document_template_framework::*;
    }
}

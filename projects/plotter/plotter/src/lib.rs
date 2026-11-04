//! Plotter 门面 — 只做稳定再导出。
//!
//! 每个模块是通向某个 `plotter-*` crate 的薄窗口，本 crate 不定义类型或业务逻辑。

#![deny(missing_docs)]

/// 共享 diagram/chart 原语；GoG 侧 ID / 诊断在 `types::plot`。
pub mod types {
    pub use plotter_types::*;
}

/// Grammar of Graphics 用的表 / 图 / 树数据。
pub mod data {
    pub use plotter_data::*;
}

/// `PlotSpec` 与编译。
pub mod grammar {
    pub use plotter_grammar::*;
}

/// GoG 后端无关 Scene IR（Arena + 稳定 ID）。
pub mod view {
    pub use plotter_view::*;
}

/// 面板与图 / 树布局。
pub mod layout {
    pub use plotter_layout::*;
}

/// CPU 渲染合同与帧报告。
pub mod renderer {
    pub use plotter_renderer::*;

    /// SVG 矢量后端。
    #[cfg(feature = "svg")]
    pub mod svg {
        pub use plotter_renderer_svg::*;
    }

    /// WGPU / WebGPU 后端。
    #[cfg(feature = "wgpu")]
    pub mod wgpu {
        pub use plotter_renderer_wgpu::*;
    }
}

/// 通用矢量场景（semantic + display）。
pub mod scene {
    pub use plotter_scene::*;
}

/// `DiagramSpec` → plan → scene 管道（不含语言前端）。
pub mod diagram {
    pub use plotter_diagram::*;
}

/// 图表 Spec 与 Vega-Lite lowering。
pub mod chart {
    pub use plotter_chart::*;
}

/// 运行时状态、事件与命中测试。
pub mod runtime {
    pub use plotter_runtime::*;
}

/// 场景对应、过渡与时间轴。
pub mod motion {
    pub use plotter_motion::*;
}

/// 降到 `DiagramSpec` 的语言前端。
pub mod input {
    /// Mermaid flowchart / sequence / class。
    pub mod mermaid {
        pub use plotter_input_mermaid::*;
    }

    /// Graphviz DOT 白名单子集。
    pub mod dot {
        pub use plotter_input_dot::*;
    }

    /// PlantUML class / sequence 子集。
    pub mod plantuml {
        pub use plotter_input_plantuml::*;
    }

    /// TikZ `tikzpicture` 子集，直接降到 scene。
    pub mod tikz {
        pub use plotter_input_tikz::*;
    }
}

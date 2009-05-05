//! `PlotSpec` 校验。

use plotter_data::ColumnTable;
use plotter_types::plot::{Diagnostic, DiagnosticCode, Result};

use crate::{
    coordinate::CoordinateSpec,
    facet::FacetSpec,
    layer::{GeomSpec, GeomText, StatSpec},
    mapping::Mapping,
    plot::{DataRef, PlotSpec},
    scale::ScaleKind,
};

/// 校验图规格是否可进入后续编译。
pub fn validate_plot(spec: &PlotSpec) -> Result<()> {
    if spec.layers.is_empty() {
        return Err(Diagnostic::error(DiagnosticCode::InvalidLayer, "至少需要一个图层"));
    }

    match &spec.coordinates {
        CoordinateSpec::Cartesian2d(_) => validate_2d(spec)?,
        CoordinateSpec::Cartesian3d(_) => validate_3d(spec)?,
        CoordinateSpec::GraphSpace(_) => validate_graph(spec)?,
        CoordinateSpec::TreeSpace(_) => validate_tree(spec)?,
    }

    Ok(())
}

fn validate_2d(spec: &PlotSpec) -> Result<()> {
    let DataRef::Table(table) = &spec.data
    else {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "二维图需要 ColumnTable"));
    };
    table.validate()?;

    if spec.facets.is_some() && matches!(spec.data, DataRef::Grid(_)) {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "分面尚不支持 GridData"));
    }

    for scale in &spec.scales {
        match scale.kind {
            ScaleKind::Continuous | ScaleKind::Log10 => {}
        }
        if let Some(domain) = scale.domain
            && matches!(scale.kind, ScaleKind::Log10)
            && (domain.min <= 0.0 || domain.max <= 0.0)
        {
            return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "log10 scale 的 domain 必须为正")
                .with_param("aesthetic", scale.aesthetic.clone()));
        }
    }

    if let Some(facets) = &spec.facets {
        validate_facets(table, facets)?;
    }

    for (index, layer) in spec.layers.iter().enumerate() {
        if layer.data.is_some() {
            return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "尚未支持层级覆盖数据")
                .with_param("layer", index.to_string()));
        }
        match layer.stat {
            StatSpec::Identity => {}
        }
        match &layer.geom {
            GeomSpec::Line(_) | GeomSpec::Point(_) | GeomSpec::Bar(_) => {
                validate_xy_layer(table, &spec.mapping, &layer.mapping, index, layer_name(&layer.geom))?
            }
            GeomSpec::Text(text) => validate_text_layer(table, &spec.mapping, &layer.mapping, text, index)?,
            GeomSpec::Surface(_) | GeomSpec::Point3d(_) => {
                return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "三维 geom 需要 cartesian3d")
                    .with_param("layer", index.to_string()));
            }
            GeomSpec::Node(_) | GeomSpec::Edge(_) | GeomSpec::TreeNode(_) | GeomSpec::TreeEdge(_) => {
                return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "图/树 geom 需要对应坐标系")
                    .with_param("layer", index.to_string()));
            }
        }
        if matches!(layer.geom, GeomSpec::Bar(_))
            && spec.scales.iter().any(|s| s.aesthetic == "y" && matches!(s.kind, ScaleKind::Log10))
        {
            return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "geom_bar 暂不支持 log10 y scale")
                .with_param("layer", index.to_string()));
        }
    }

    Ok(())
}

fn validate_3d(spec: &PlotSpec) -> Result<()> {
    if spec.facets.is_some() {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "三维图暂不支持分面"));
    }

    for (index, layer) in spec.layers.iter().enumerate() {
        if layer.data.is_some() {
            return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "尚未支持层级覆盖数据")
                .with_param("layer", index.to_string()));
        }
        match layer.stat {
            StatSpec::Identity => {}
        }
        match &layer.geom {
            GeomSpec::Surface(_) => {
                let DataRef::Grid(grid) = &spec.data
                else {
                    return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "geom_surface 需要 GridData")
                        .with_param("layer", index.to_string()));
                };
                grid.validate()?;
            }
            GeomSpec::Point3d(_) => {
                let DataRef::Table(table) = &spec.data
                else {
                    return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "geom_point3d 需要 ColumnTable")
                        .with_param("layer", index.to_string()));
                };
                table.validate()?;
                validate_xyz_layer(table, &spec.mapping, &layer.mapping, index)?;
            }
            _ => {
                return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "三维坐标系仅支持 surface / point3d")
                    .with_param("layer", index.to_string()));
            }
        }
    }
    Ok(())
}

fn validate_facets(table: &ColumnTable, facets: &FacetSpec) -> Result<()> {
    match facets {
        FacetSpec::Wrap { column, ncol } => {
            table.string_column(column)?;
            if let Some(0) = ncol {
                return Err(Diagnostic::error(DiagnosticCode::ValidationFailed, "facet ncol 不能为 0"));
            }
        }
    }
    Ok(())
}

fn layer_name(geom: &GeomSpec) -> &'static str {
    match geom {
        GeomSpec::Line(_) => "geom_line",
        GeomSpec::Point(_) => "geom_point",
        GeomSpec::Bar(_) => "geom_bar",
        GeomSpec::Text(_) => "geom_text",
        GeomSpec::Surface(_) => "geom_surface",
        GeomSpec::Point3d(_) => "geom_point3d",
        GeomSpec::Node(_) => "geom_node",
        GeomSpec::Edge(_) => "geom_edge",
        GeomSpec::TreeNode(_) => "geom_tree_node",
        GeomSpec::TreeEdge(_) => "geom_tree_edge",
    }
}

fn validate_graph(spec: &PlotSpec) -> Result<()> {
    let DataRef::Graph(graph) = &spec.data
    else {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "graph-space 需要 GraphData"));
    };
    graph.validate()?;
    if spec.facets.is_some() {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "图布局暂不支持分面"));
    }
    for (index, layer) in spec.layers.iter().enumerate() {
        if layer.data.is_some() {
            return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "尚未支持层级覆盖数据")
                .with_param("layer", index.to_string()));
        }
        match layer.stat {
            StatSpec::Identity => {}
        }
        match &layer.geom {
            GeomSpec::Node(_) | GeomSpec::Edge(_) => {}
            _ => {
                return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "graph-space 仅支持 geom_node / geom_edge")
                    .with_param("layer", index.to_string()));
            }
        }
    }
    Ok(())
}

fn validate_tree(spec: &PlotSpec) -> Result<()> {
    let DataRef::Tree(tree) = &spec.data
    else {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "tree-space 需要 TreeData"));
    };
    tree.validate()?;
    if spec.facets.is_some() {
        return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "树布局暂不支持分面"));
    }
    for (index, layer) in spec.layers.iter().enumerate() {
        if layer.data.is_some() {
            return Err(Diagnostic::error(DiagnosticCode::UnsupportedSpec, "尚未支持层级覆盖数据")
                .with_param("layer", index.to_string()));
        }
        match layer.stat {
            StatSpec::Identity => {}
        }
        match &layer.geom {
            GeomSpec::TreeNode(_) | GeomSpec::TreeEdge(_) => {}
            _ => {
                return Err(Diagnostic::error(
                    DiagnosticCode::UnsupportedSpec,
                    "tree-space 仅支持 geom_tree_node / geom_tree_edge",
                )
                .with_param("layer", index.to_string()));
            }
        }
    }
    Ok(())
}

fn validate_xy_layer(
    table: &ColumnTable,
    plot_mapping: &Mapping,
    layer_mapping: &Mapping,
    layer_index: usize,
    geom_name: &str,
) -> Result<()> {
    let mapping = plot_mapping.merge(layer_mapping);
    let x = mapping.x.as_ref().ok_or_else(|| {
        Diagnostic::error(DiagnosticCode::MissingMapping, format!("{geom_name} 需要 x mapping"))
            .with_param("layer", layer_index.to_string())
            .with_param("aesthetic", "x")
    })?;
    let y = mapping.y.as_ref().ok_or_else(|| {
        Diagnostic::error(DiagnosticCode::MissingMapping, format!("{geom_name} 需要 y mapping"))
            .with_param("layer", layer_index.to_string())
            .with_param("aesthetic", "y")
    })?;
    table.float_column(x.column_name())?;
    table.float_column(y.column_name())?;
    Ok(())
}

fn validate_xyz_layer(table: &ColumnTable, plot_mapping: &Mapping, layer_mapping: &Mapping, layer_index: usize) -> Result<()> {
    validate_xy_layer(table, plot_mapping, layer_mapping, layer_index, "geom_point3d")?;
    let mapping = plot_mapping.merge(layer_mapping);
    let z = mapping.z.as_ref().ok_or_else(|| {
        Diagnostic::error(DiagnosticCode::MissingMapping, "geom_point3d 需要 z mapping")
            .with_param("layer", layer_index.to_string())
            .with_param("aesthetic", "z")
    })?;
    table.float_column(z.column_name())?;
    Ok(())
}

fn validate_text_layer(
    table: &ColumnTable,
    plot_mapping: &Mapping,
    layer_mapping: &Mapping,
    text: &GeomText,
    layer_index: usize,
) -> Result<()> {
    validate_xy_layer(table, plot_mapping, layer_mapping, layer_index, "geom_text")?;
    let mapping = plot_mapping.merge(layer_mapping);
    match (&mapping.label, &text.text) {
        (Some(label), _) => {
            table.string_column(label.column_name())?;
        }
        (None, Some(_)) => {}
        (None, None) => {
            return Err(Diagnostic::error(DiagnosticCode::MissingMapping, "geom_text 需要 label mapping 或常量 text")
                .with_param("layer", layer_index.to_string())
                .with_param("aesthetic", "label"));
        }
    }
    Ok(())
}

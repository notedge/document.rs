//! `PlotSpec` 与数据引用。

use plotter_data::{ColumnTable, GraphData, GridData, TreeData};

use crate::{
    coordinate::CoordinateSpec, facet::FacetSpec, layer::LayerSpec, mapping::Mapping, scale::ScaleSpec, theme::ThemeSpec,
};

/// 图级数据引用。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DataRef {
    /// 列式表。
    Table(ColumnTable),
    /// 规则网格（曲面）。
    Grid(GridData),
    /// 图。
    Graph(GraphData),
    /// 树。
    Tree(TreeData),
}

/// 声明式图形规格。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlotSpec {
    /// 主数据。
    pub data: DataRef,
    /// 图级 mapping。
    pub mapping: Mapping,
    /// 图层。
    pub layers: Vec<LayerSpec>,
    /// Scale 列表。
    pub scales: Vec<ScaleSpec>,
    /// 坐标系。
    pub coordinates: CoordinateSpec,
    /// 可选分面。
    pub facets: Option<FacetSpec>,
    /// 主题。
    pub theme: ThemeSpec,
}

impl PlotSpec {
    /// 由列式表构造空白图规格（尚无图层）。
    pub fn new(table: ColumnTable) -> Self {
        Self {
            data: DataRef::Table(table),
            mapping: Mapping::default(),
            layers: Vec::new(),
            scales: vec![ScaleSpec::continuous_x(), ScaleSpec::continuous_y()],
            coordinates: CoordinateSpec::default(),
            facets: None,
            theme: ThemeSpec::default(),
        }
    }

    /// 由规则网格构造三维图规格。
    pub fn from_grid(grid: GridData) -> Self {
        Self {
            data: DataRef::Grid(grid),
            mapping: Mapping::default(),
            layers: Vec::new(),
            scales: Vec::new(),
            coordinates: CoordinateSpec::cartesian_3d(),
            facets: None,
            theme: ThemeSpec::default(),
        }
    }

    /// 由图数据构造（默认圆周布局）。
    pub fn from_graph(graph: GraphData) -> Self {
        Self {
            data: DataRef::Graph(graph),
            mapping: Mapping::default(),
            layers: Vec::new(),
            scales: Vec::new(),
            coordinates: CoordinateSpec::graph_space(),
            facets: None,
            theme: ThemeSpec::default(),
        }
    }

    /// 由树数据构造（默认 tidy 布局）。
    pub fn from_tree(tree: TreeData) -> Self {
        Self {
            data: DataRef::Tree(tree),
            mapping: Mapping::default(),
            layers: Vec::new(),
            scales: Vec::new(),
            coordinates: CoordinateSpec::tree_space(),
            facets: None,
            theme: ThemeSpec::default(),
        }
    }

    /// 设置图级 mapping。
    pub fn mapping(mut self, mapping: Mapping) -> Self {
        self.mapping = mapping;
        self
    }

    /// 追加图层。
    pub fn layer(mut self, layer: LayerSpec) -> Self {
        self.layers.push(layer);
        self
    }

    /// 替换 scales（同 aesthetic 后者覆盖前者的意图由调用方控制）。
    pub fn scales(mut self, scales: Vec<ScaleSpec>) -> Self {
        self.scales = scales;
        self
    }

    /// 追加或替换同名 aesthetic 的 scale。
    pub fn scale(mut self, scale: ScaleSpec) -> Self {
        if let Some(existing) = self.scales.iter_mut().find(|s| s.aesthetic == scale.aesthetic) {
            *existing = scale;
        }
        else {
            self.scales.push(scale);
        }
        self
    }

    /// 设置坐标系。
    pub fn coordinates(mut self, coordinates: CoordinateSpec) -> Self {
        self.coordinates = coordinates;
        self
    }

    /// 设置分面。
    pub fn facets(mut self, facets: FacetSpec) -> Self {
        self.facets = Some(facets);
        self
    }

    /// 设置主题。
    pub fn theme(mut self, theme: ThemeSpec) -> Self {
        self.theme = theme;
        self
    }
}

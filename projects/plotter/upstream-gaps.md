# 绘图输入的上游缺口与接受集合

本目录的 `plotter.input.*` 只做「源文本 → `plotter_diagram::DiagramSpec` → `DisplayScene`」的降级，不做语义猜测。机器可读的接受集合见同目录 [`input-contracts.json`](input-contracts.json)，本文只解释为什么某项被拒绝或被称为损失。

## 一、沉默缺口审计

审计方法：逐项读 `oak-dot` / `oak-mermaid` / `oak-plantuml` 的 AST 枚举，与三个前端 `match` 分支和结构体字段逐字段比对。Rust 的穷尽匹配保证每个枚举变体都有分支，因此「沉默缺口」只可能来自三种位置：结构体字段被忽略、上游 builder 在建 AST 前就丢弃构造、前端在解析前对源文本做了错误分类。

| 前端 | 枚举覆盖 | 被忽略的字段 | 结论 |
|---|---|---|---|
| `plotter.input.dot` | `Statement` 五变体、`EdgeOperand` 两变体、`AttributeTarget` 三变体、`Compass` 九变体全部有分支 | `Graph.id` | 图名只用于命名，不进入场景，属显式 no-op。`digraph pipeline` 的正例已固化 |
| `plotter.input.mermaid` | `MermaidDiagram` 三变体、`FlowElement` 三变体、`NodeShape` 七变体、`EdgeStyle` 四变体、`ParticipantKind` 两变体、`MessageArrow` 四变体、`RelationKind` 五变体全部有分支 | `oak_uml::Member.kind` | 方法身份由成员文本里的 `(` 携带，`kind` 是派生值。`class_method_members_keep_their_parentheses` 钉住这一点 |
| `plotter.input.plantuml` | `PlantUmlDiagram` 两变体、`ClassKeyword` 四变体、`MemberVisibility` 四变体、`ClassRelationKind` 六变体、`MessageArrow` 四变体、`ParticipantKind` 两变体全部有分支 | `ClassMember.is_method` | 同 Mermaid，方法身份由文本的 `(` 携带，`is_method` 是派生值 |

审计中修掉的两处真实沉默缺口：

1. `plotter.input.mermaid` 的 sequence 守卫原先先判箭头再判语句关键字，一行 `Note over A,B: click -> here` 因为正文含 `->` 被判成消息，而 `oak-mermaid` 会丢弃整条 note，结果是「AST 有、我们丢内容、还报成功」。现在先按 `DROPPED_SEQUENCE_STATEMENTS` 判语句起始，再判箭头，拒绝时带整行字节范围。
2. 非 flowchart/sequence/class 的 Mermaid 家族原先落到 `ast-unavailable`（整个正文范围，无法定位）。现在在解析前按 `MermaidRoot` 首个家族头判族，返回 `davinci.diagram.unsupported` 并指向该头所在行。

## 二、上游 AST 缺口（需要 `oak-*` 扩节点模型）

| 输入 | 构造 | 现状 | 缺什么节点模型 | 需要什么语义 |
|---|---|---|---|---|
| `oak-mermaid` | `stateDiagram` / `stateDiagram-v2` | 拒绝于族头 | 状态节点、复合状态与并发区域、`[*]` 起止伪状态 | 状态集合、迁移源与目标、迁移标签、进入与退出动作、区域层级 |
| `oak-mermaid` | `erDiagram` | 拒绝于族头 | 实体、属性（类型与主键/外键标记）、关系基数（`||`、`o{` 等 Crow's Foot 记号） | 实体属性表、两端基数、识别与非识别关系、关系标签 |
| `oak-mermaid` | `classDiagram` 的 `<<interface>>` 与 `<<abstract>>` 原型 | 降成普通类 | `oak-uml::Class` 无原型字段，`ClassDiagram::from` 把接口的成员搬进类后丢弃身份 | 保留原型标记，类名与原型分开存放 |
| `oak-mermaid` | `classDiagram` 关系两端的多重性（`A "1" --> "many" B`） | 上游未建模 | `oak-uml::Relation` 无多重性字段 | 两端各一个可选的多重性文本 |
| `oak-mermaid` | `sequenceDiagram` 的 `activate` / `deactivate` 与 `+` / `-` 简写 | 拒绝于语句行 | `SequenceDiagram` 无激活区间，PlantUML 的同名字段可参照 | 激活所属 participant、起始与结束消息序号 |
| `oak-plantuml` | `activity`（旧语法）与 `activityDiagram`（新语法） | 拒绝于族头 | 活动节点、判断与合并、并行分叉与汇合、泳道、起止节点 | 控制流边与守卫条件、泳道归属、分叉汇合配对 |
| `oak-plantuml` | `state` | 拒绝于族头 | 状态节点、复合状态、并发区域 | 迁移源与目标、迁移标签、状态内动作 |
| `oak-plantuml` | `component` / `deployment` / `usecase` 等其余家族 | 拒绝于族头 | 组件、接口、端口、包、参与者、节点 | 组件与接口的提供/需求关系、嵌套归属 |

补齐方式：上游 AST 出现上述节点与关系后，本层只在既有 `DiagramSpec` 上增加降级分支，`DiagramSpec` 需要新字段时先报 `plotter.diagram` 设计缺口，不在前端私造中间结构。

## 三、下游场景模型缺口（需要 `plotter.diagram` / `plotter.scene` 扩模型）

| 输入 | 构造 | 现状 | 缺什么模型 | 需要什么语义 |
|---|---|---|---|---|
| `oak-dot` | `record` / `mrecord` 节点 | 拒绝于属性 | 字段端口（field port）与端口级锚点 | 端口顺序与相对位置、边按端口锚定、端口分隔符与对齐 |
| `oak-dot` | 匿名子图、嵌套子图、子图作边操作数 | 拒绝于语句 | 无 id 的簇、簇的嵌套层级、簇整体作为边端点 | 簇 id 自动生成规则、嵌套簇包围框、簇端点展开为成员端点 |

`record` 长期拒绝的理由：下游只有节点级锚点，没有端口模型，强行降级会把端口压平成普通方框，属于近似而非降级。

## 四、明确不接的输入

| 输入 | 决策 | 理由 |
|---|---|---|
| TikZ 的 PGFPlots `\begin{axis}` | 不接 | 坐标轴、刻度、图例与数据映射属于 `plotter.chart`，接进来会形成两套图表模型 |
| TikZ 的宿主注入（宏展开、外部图片、`\input`） | 不接 | 属宿主与排版层的职责，不是输入适配器的降级范围 |
| `oak-mermaid` 的 `gantt` / `pie` / `journey` 等 | 不接 | 时间轴与占比是 `plotter.chart` 的领域，不走图元降级 |
| `oak-plantuml` 的 `note` / `loop` / `alt` / `else` 等注释与组合片段 | 不接 | 需要场景层的浮层注释与组合框语义，`DiagramSpec` 无对应模型 |

## 五、真实语料的归属

覆盖矩阵、语料对比与像素基准不放 `document.rs`，由下游 harness（`panduck` 侧的语料家底）承接。本仓只保留冒烟：能编译、最小 happy path、关键拒绝路径。

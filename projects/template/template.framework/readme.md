# template-framework

`template.framework`：构建框架的模板合同。框架决定页面数据、选择入口模板并编排
输入引擎，但**不定义模板语法**；渲染必须委托给 `FrameworkCapability::engines` 声明的
`TemplateEngine`。

首个登记是 `template.framework.doki`，实现留在 Doki 仓，本 crate 只交付接口形状。

```rust,ignore
use template_framework::{FrameworkCapability, FrameworkRequest, FrameworkOutput, TemplateFramework};
use template::{RenderOptions, TemplateStatus};

struct Doki;

impl TemplateFramework for Doki {
    fn id(&self) -> &'static str { "doki" }

    fn capability(&self) -> FrameworkCapability {
        FrameworkCapability {
            id: "doki",
            status: TemplateStatus::Ready,
            engines: &["dejavu"],
            contract: "doki/v0",
        }
    }

    fn render(&self, request: &FrameworkRequest<'_>) -> Result<FrameworkOutput, TemplateError> {
        // 解析 entry、准备页面数据，再调用 template.input.dejavu。
        # unimplemented!()
    }
}
```

约束：

- 宿主扩展（i18n registry、主题解析、布局候选顺序）由 framework 实现自己持有，不进入本合同。
- framework 专有失败发 `template.framework.<id>.<failure>`；借道输入引擎的失败保留引擎码。
- 本 crate 不新增 N-API；framework 的 N-API 表面由持有它的产品仓提供。

use serde_json::json;
use template::{MapTemplateLoader, RenderOptions, RenderRequest, TemplateEngine, TemplateId, TemplateLoader, TemplateRoot};
use template_framework::{FrameworkCapability, FrameworkOutput, FrameworkRequest, TemplateFramework};
use template_input_hbs::HbsEngine;

/// A minimal framework: resolve the entry through the shared loader, then
/// delegate the whole render to the input engine.
struct StubFramework;

impl TemplateFramework for StubFramework {
    fn id(&self) -> &'static str {
        "stub"
    }

    fn capability(&self) -> FrameworkCapability {
        FrameworkCapability { id: "stub", status: template::TemplateStatus::Ready, engines: &["hbs"], contract: "stub/v0" }
    }

    fn render(&self, request: &FrameworkRequest<'_>) -> Result<FrameworkOutput, template::TemplateError> {
        let extensions = ["", ".hbs"];
        let id = request.loader.resolve(request.entry, None, &extensions)?;
        let source = request.loader.source(&id)?;
        let output = HbsEngine.render(&RenderRequest {
            source,
            entry: Some(&id),
            loader: request.loader,
            data: request.data,
            options: request.options,
        })?;
        Ok(FrameworkOutput { text: output.text, content: HbsEngine.capability().content, dependencies: output.dependencies })
    }
}

fn loader() -> MapTemplateLoader {
    MapTemplateLoader::new().with_root(
        TemplateRoot::new("theme", 0)
            .with_file("page.hbs", "<h1>{{ title }}</h1>")
            .with_file("nav.hbs", "<nav>{{ title }}</nav>"),
    )
}

#[test]
fn framework_resolves_the_entry_and_delegates_to_the_input_engine() {
    let loader = loader();
    let options = RenderOptions::default();
    let output = StubFramework
        .render(&FrameworkRequest { entry: "page", loader: &loader, data: &json!({ "title": "Hello" }), options: &options })
        .expect("framework render");
    assert_eq!(output.text, "<h1>Hello</h1>");
    assert_eq!(output.dependencies, vec![TemplateId::new("theme", "page.hbs")]);
}

#[test]
fn framework_capability_names_its_engines_and_contract() {
    let capability = StubFramework.capability();
    assert_eq!(capability.id, "stub");
    assert_eq!(capability.engines, &["hbs"]);
    assert_eq!(capability.contract, "stub/v0");
}

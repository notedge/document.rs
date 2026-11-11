use serde_json::json;
use template::{
    MapTemplateLoader, RenderLimits, RenderOptions, RenderOutput, RenderRequest, TemplateEngine, TemplateError, TemplateId,
    TemplateRoot, codes,
};
use template_input_hbs::{HbsEngine, codes as hbs_codes};

fn loader() -> MapTemplateLoader {
    MapTemplateLoader::new().with_root(
        TemplateRoot::new("map", 0)
            .with_file("hello.hbs", "Hello {{name}}")
            .with_file("pages/list.hbs", "{{#each items}}[{{@index}}:{{this}}]{{/each}}")
            .with_file("loop.hbs", "{{> loop}}"),
    )
}

fn render_with(
    source: &str,
    data: &serde_json::Value,
    entry: Option<&TemplateId>,
    options: RenderOptions,
) -> Result<RenderOutput, TemplateError> {
    HbsEngine.render(&RenderRequest { source, entry, loader: &loader(), data, options: &options })
}

fn render(source: &str, data: serde_json::Value) -> String {
    render_with(source, &data, None, RenderOptions::default()).expect("render").text
}

fn code(result: Result<RenderOutput, TemplateError>) -> String {
    result.map_or_else(|error| error.code().to_string(), |_| String::new())
}

#[test]
fn interpolation_escapes_and_triple_stache_does_not() {
    assert_eq!(render("{{value}}", json!({ "value": "<b>&\"'=`" })), "&lt;b&gt;&amp;&quot;&#x27;&#x3D;&#x60;");
    assert_eq!(render("{{{value}}}", json!({ "value": "<b>&\"'=`" })), "<b>&\"'=`");
}

#[test]
fn if_else_and_else_if_choose_one_branch() {
    assert_eq!(render("{{#if a}}A{{else}}B{{/if}}", json!({ "a": true })), "A");
    assert_eq!(render("{{#if a}}A{{else}}B{{/if}}", json!({ "a": 0 })), "B");
    assert_eq!(render("{{#unless a}}A{{else}}B{{/unless}}", json!({ "a": "" })), "A");
    assert_eq!(render("{{#if a}}A{{else if b}}B{{else}}C{{/if}}", json!({ "a": false, "b": true })), "B");
    assert_eq!(render("{{#if a}}A{{else if b}}B{{else}}C{{/if}}", json!({ "a": false, "b": false })), "C");
}

#[test]
fn empty_collections_are_truthy_in_hbs() {
    assert_eq!(render("{{#if items}}yes{{else}}no{{/if}}", json!({ "items": [] })), "yes");
}

#[test]
fn each_exposes_index_and_first_last() {
    let data = json!({ "items": ["a", "b"] });
    assert_eq!(
        render("{{#each items}}{{@index}}:{{this}}:{{@first}}:{{@last}};{{/each}}", data),
        "0:a:true:false;1:b:false:true;"
    );
    assert_eq!(render("{{#each items}}x{{else}}empty{{/each}}", json!({ "items": [] })), "empty");
}

#[test]
fn each_over_object_uses_key_and_sorted_order() {
    let data = json!({ "items": { "b": 2, "a": 1 } });
    assert_eq!(render("{{#each items}}{{@key}}={{this}};{{/each}}", data), "a=1;b=2;");
}

#[test]
fn with_scopes_the_context_and_honors_parent_paths() {
    let data = json!({ "outer": "O", "inner": { "name": "N" } });
    assert_eq!(render("{{#with inner}}{{name}}/{{../outer}}{{/with}}", data.clone()), "N/O");
    assert_eq!(render("{{#with inner}}{{@root.outer}}{{/with}}", data.clone()), "O");
    assert_eq!(render("{{#with missing}}x{{else}}none{{/with}}", data), "none");
}

#[test]
fn partial_renders_current_context_and_records_dependency() {
    let data = json!({ "name": "world" });
    let entry = TemplateId::new("map", "main.hbs");
    let output = render_with("{{> hello}}", &data, Some(&entry), RenderOptions::default()).expect("render");
    assert_eq!(output.text, "Hello world");
    assert_eq!(output.dependencies.first().map(ToString::to_string), Some("map:main.hbs".to_string()));
    assert_eq!(output.dependencies.last().map(ToString::to_string), Some("map:hello.hbs".to_string()));
}

#[test]
fn partial_uses_relative_references() {
    let data = json!({ "items": [1, 2] });
    let entry = TemplateId::new("map", "pages/home.hbs");
    let output = render_with("{{> ./list}}", &data, Some(&entry), RenderOptions::default()).expect("render");
    assert_eq!(output.text, "[0:1][1:2]");
}

#[test]
fn strict_undefined_is_reported() {
    let options = RenderOptions { strict_undefined: true, ..RenderOptions::default() };
    assert_eq!(code(render_with("{{missing}}", &json!({}), None, options)), codes::UNDEFINED_VARIABLE);
    assert_eq!(render("{{missing}}", json!({})), "");
}

#[test]
fn helper_calls_and_unknown_blocks_fail_closed() {
    assert_eq!(
        code(render_with("{{upper name}}", &json!({ "name": "x" }), None, RenderOptions::default())),
        hbs_codes::UNSUPPORTED_HELPER
    );
    assert_eq!(
        code(render_with("{{#each items skip=1}}{{/each}}", &json!({ "items": [] }), None, RenderOptions::default())),
        hbs_codes::UNSUPPORTED_HELPER
    );
    assert_eq!(
        code(render_with("{{#repeat n}}x{{/repeat}}", &json!({ "n": 2 }), None, RenderOptions::default())),
        hbs_codes::UNSUPPORTED_BLOCK
    );
    assert_eq!(
        code(render_with("{{> hello x=1}}", &json!({}), None, RenderOptions::default())),
        hbs_codes::UNSUPPORTED_CONSTRUCT
    );
}

#[test]
fn whitespace_control_trims_one_side() {
    assert_eq!(render("a  {{~x~}}  b", json!({ "x": "X" })), "aXb");
    assert_eq!(render("a\n{{~x}}", json!({ "x": "X" })), "aX");
}

#[test]
fn budgets_are_hard_ceilings() {
    let options =
        RenderOptions { limits: RenderLimits { max_output_bytes: 4, ..RenderLimits::default() }, ..RenderOptions::default() };
    assert_eq!(code(render_with("hello", &json!({}), None, options)), codes::OUTPUT_BUDGET);

    let options =
        RenderOptions { limits: RenderLimits { max_source_bytes: 4, ..RenderLimits::default() }, ..RenderOptions::default() };
    assert_eq!(code(render_with("hello", &json!({}), None, options)), codes::SOURCE_BUDGET);

    let options =
        RenderOptions { limits: RenderLimits { max_steps: 1, ..RenderLimits::default() }, ..RenderOptions::default() };
    assert_eq!(code(render_with("{{a}}{{b}}", &json!({}), None, options)), codes::STEP_BUDGET);

    let options =
        RenderOptions { limits: RenderLimits { max_include_depth: 1, ..RenderLimits::default() }, ..RenderOptions::default() };
    assert_eq!(code(render_with("{{> loop}}", &json!({}), None, options)), codes::INCLUDE_DEPTH);
}

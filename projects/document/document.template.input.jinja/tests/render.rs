use document_template::{
    MapTemplateLoader, RenderLimits, RenderOptions, RenderOutput, RenderRequest, TemplateEngine, TemplateError, TemplateId,
    TemplateRoot, codes,
};
use document_template_input_jinja::{JinjaEngine, codes as jinja_codes};
use serde_json::json;

fn loader() -> MapTemplateLoader {
    MapTemplateLoader::new().with_root(
        TemplateRoot::new("map", 0)
            .with_file("hi.jinja", "Hello {{ name }}")
            .with_file("pages/row.jinja", "{{ row }}")
            .with_file("loop.jinja", "{% include \"loop.jinja\" %}"),
    )
}

fn render_with(
    source: &str,
    data: &serde_json::Value,
    entry: Option<&TemplateId>,
    options: RenderOptions,
) -> Result<RenderOutput, TemplateError> {
    JinjaEngine.render(&RenderRequest { source, entry, loader: &loader(), data, options: &options })
}

fn render(source: &str, data: serde_json::Value) -> String {
    render_with(source, &data, None, RenderOptions::default()).expect("render").text
}

fn code(result: Result<RenderOutput, TemplateError>) -> String {
    result.map_or_else(|error| error.code().to_string(), |_| String::new())
}

#[test]
fn output_escapes_and_safe_or_escape_bypasses() {
    let data = json!({ "value": "<b>\"'&" });
    assert_eq!(render("{{ value }}", data.clone()), "&lt;b&gt;&#34;&#39;&amp;");
    assert_eq!(render("{{ value|safe }}", data.clone()), "<b>\"'&");
    assert_eq!(render("{{ value|escape }}", data), "&lt;b&gt;&#34;&#39;&amp;");
}

#[test]
fn if_elif_and_else_choose_one_branch() {
    let data = json!({ "a": false, "b": true });
    assert_eq!(render("{% if a %}A{% elif b %}B{% else %}C{% endif %}", data.clone()), "B");
    assert_eq!(render("{% if a %}A{% elif b %}B{% else %}C{% endif %}", json!({ "a": true, "b": true })), "A");
    assert_eq!(render("{% if a %}A{% elif b %}B{% else %}C{% endif %}", json!({ "a": false, "b": false })), "C");
}

#[test]
fn for_exposes_loop_metadata_and_else() {
    let data = json!({ "items": ["a", "b"] });
    assert_eq!(
        render(
            "{% for x in items %}{{ loop.index }}/{{ loop.index0 }}/{{ loop.first }}/{{ loop.last }}/{{ loop.length }}:{{ x }};{% endfor %}",
            data
        ),
        "1/0/True/False/2:a;2/1/False/True/2:b;"
    );
    assert_eq!(render("{% for x in items %}x{% else %}empty{% endfor %}", json!({ "items": [] })), "empty");
}

#[test]
fn for_over_object_uses_sorted_keys() {
    let data = json!({ "obj": { "b": 2, "a": 1 } });
    assert_eq!(render("{% for k in obj %}{{ k }};{% endfor %}", data), "a;b;");
}

#[test]
fn set_is_scoped_to_its_block() {
    let data = json!({ "name": "world", "items": [1, 2] });
    assert_eq!(render("{% set who = name|upper %}{{ who }}", data.clone()), "WORLD");
    assert_eq!(render("{% for x in items %}{% set last = x %}{% endfor %}{{ last }}", data), "None");
}

#[test]
fn include_shares_context_and_records_dependency() {
    let data = json!({ "name": "world" });
    let entry = TemplateId::new("map", "main.jinja");
    let output = render_with("{% include \"hi.jinja\" %}", &data, Some(&entry), RenderOptions::default()).expect("render");
    assert_eq!(output.text, "Hello world");
    assert_eq!(output.dependencies.first().map(ToString::to_string), Some("map:main.jinja".to_string()));
    assert_eq!(output.dependencies.last().map(ToString::to_string), Some("map:hi.jinja".to_string()));
}

#[test]
fn include_uses_relative_references() {
    let data = json!({ "row": "R" });
    let entry = TemplateId::new("map", "pages/home.jinja");
    let output = render_with("{% include \"./row.jinja\" %}", &data, Some(&entry), RenderOptions::default()).expect("render");
    assert_eq!(output.text, "R");
}

#[test]
fn filters_cover_the_declared_set() {
    let data = json!({ "name": "World", "items": [1, 2], "obj": { "a": 1, "b": 2 }, "value": null });
    assert_eq!(render("{{ \"abc\"|upper }}", json!({})), "ABC");
    assert_eq!(render("{{ name|lower }}", data.clone()), "world");
    assert_eq!(render("{{ \" a \"|trim }}", json!({})), "a");
    assert_eq!(render("{{ \"abc\"|length }}", json!({})), "3");
    assert_eq!(render("{{ items|length }}", data.clone()), "2");
    assert_eq!(render("{{ obj|length }}", data.clone()), "2");
    assert_eq!(render("{{ missing|default(\"x\") }}", data.clone()), "x");
    assert_eq!(render("{{ value|default(\"x\") }}", data.clone()), "x");
    assert_eq!(render("{{ items|join(\"-\") }}", data.clone()), "1-2");
    assert_eq!(render("{{ \"abc\"|replace(\"b\", \"X\") }}", json!({})), "aXc");
    assert_eq!(render("{{ name|safe }}", data), "World");
}

#[test]
fn expressions_cover_arithmetic_boolean_and_comparison() {
    assert_eq!(render("{{ 1 + 2 * 3 }}", json!({})), "7");
    assert_eq!(render("{{ 7 / 2 }}", json!({})), "3.5");
    assert_eq!(render("{{ 7 % 2 }}", json!({})), "1");
    assert_eq!(render("{{ \"a\" ~ 1 }}", json!({})), "a1");
    assert_eq!(render("{{ not false }}", json!({})), "True");
    assert_eq!(render("{{ true and false }}", json!({})), "False");
    assert_eq!(render("{{ 0 or \"x\" }}", json!({})), "x");
    assert_eq!(render("{{ items[1] }}", json!({ "items": ["a", "b"] })), "b");
    assert_eq!(render("{{ obj.key }}", json!({ "obj": { "key": "v" } })), "v");
    assert_eq!(render("{{ obj[\"key\"] }}", json!({ "obj": { "key": "v" } })), "v");
}

#[test]
fn comparison_is_json_structural() {
    assert_eq!(render("{% if a == b %}yes{% else %}no{% endif %}", json!({ "a": 1, "b": 1 })), "yes");
    assert_eq!(render("{% if a == b %}yes{% else %}no{% endif %}", json!({ "a": 1, "b": true })), "no");
    assert_eq!(render("{% if a < b %}lt{% endif %}", json!({ "a": 1, "b": 2 })), "lt");
}

#[test]
fn containers_stringify_in_python_style() {
    assert_eq!(render("{{ v }}", json!({ "v": null })), "None");
    assert_eq!(render("{{ v }}", json!({ "v": true })), "True");
    assert_eq!(render("{{ v|safe }}", json!({ "v": ["a", 1] })), "['a', 1]");
    assert_eq!(render("{{ v|safe }}", json!({ "v": { "b": 2, "a": "x" } })), "{'a': 'x', 'b': 2}");
    assert_eq!(render("{{ v }}", json!({ "v": ["a"] })), "[&#39;a&#39;]");
    assert_eq!(render("{{ v }}", json!({ "v": "plain" })), "plain");
}

#[test]
fn comments_and_whitespace_markers_are_honored() {
    assert_eq!(render("a{# note #}b", json!({})), "ab");
    assert_eq!(render("a  {{- x -}}  b", json!({ "x": "X" })), "aXb");
    assert_eq!(render("a  {%- set x = 1 -%}  b", json!({})), "ab");
    assert_eq!(render("{%+ if true %}x{% endif %}", json!({})), "x");
}

#[test]
fn strict_undefined_is_reported() {
    let options = RenderOptions { strict_undefined: true, ..RenderOptions::default() };
    assert_eq!(code(render_with("{{ missing }}", &json!({}), None, options)), codes::UNDEFINED_VARIABLE);
    assert_eq!(render("{{ missing }}", json!({})), "None");
}

#[test]
fn unsupported_tags_and_constructs_fail_closed() {
    assert_eq!(
        code(render_with("{% block title %}{% endblock %}", &json!({}), None, RenderOptions::default())),
        jinja_codes::UNSUPPORTED_TAG
    );
    assert_eq!(
        code(render_with("{% raw %}x{% endraw %}", &json!({}), None, RenderOptions::default())),
        jinja_codes::UNSUPPORTED_TAG
    );
    assert_eq!(
        code(render_with("{{ 1 is 1 }}", &json!({}), None, RenderOptions::default())),
        jinja_codes::UNSUPPORTED_CONSTRUCT
    );
    assert_eq!(
        code(render_with("{% for a, b in items %}{% endfor %}", &json!({ "items": [] }), None, RenderOptions::default())),
        jinja_codes::UNSUPPORTED_CONSTRUCT
    );
    assert_eq!(
        code(render_with("{% for x in items if x %}{% endfor %}", &json!({ "items": [] }), None, RenderOptions::default())),
        jinja_codes::UNSUPPORTED_CONSTRUCT
    );
    assert_eq!(
        code(render_with("{% include name %}", &json!({ "name": "x" }), None, RenderOptions::default())),
        jinja_codes::UNSUPPORTED_CONSTRUCT
    );
}

#[test]
fn filter_and_operator_failures_use_stable_codes() {
    assert_eq!(code(render_with("{{ x|bogus }}", &json!({}), None, RenderOptions::default())), jinja_codes::UNKNOWN_FILTER);
    assert_eq!(
        code(render_with("{{ x|upper(\"a\") }}", &json!({}), None, RenderOptions::default())),
        jinja_codes::FILTER_ARITY
    );
    assert_eq!(code(render_with("{{ 1|upper }}", &json!({}), None, RenderOptions::default())), jinja_codes::FILTER_VALUE);
    assert_eq!(
        code(render_with("{{ \"a\" + \"b\" }}", &json!({}), None, RenderOptions::default())),
        jinja_codes::OPERATOR_TYPE
    );
    assert_eq!(
        code(render_with("{{ \"a\" < \"b\" }}", &json!({}), None, RenderOptions::default())),
        jinja_codes::OPERATOR_TYPE
    );
    assert_eq!(code(render_with("{{ 1 / 0 }}", &json!({}), None, RenderOptions::default())), jinja_codes::ARITHMETIC);
    assert_eq!(code(render_with("{{ 7 % 0 }}", &json!({}), None, RenderOptions::default())), jinja_codes::ARITHMETIC);
    assert_eq!(
        code(render_with("{{ 9223372036854775807 + 1 }}", &json!({}), None, RenderOptions::default())),
        jinja_codes::ARITHMETIC
    );
}

#[test]
fn for_over_non_iterable_values_fails_closed() {
    assert_eq!(
        code(render_with("{% for x in 1 %}x{% endfor %}", &json!({}), None, RenderOptions::default())),
        jinja_codes::ITERATION_TYPE
    );
    assert_eq!(
        code(render_with("{% for x in nothing %}x{% endfor %}", &json!({}), None, RenderOptions::default())),
        jinja_codes::ITERATION_TYPE
    );
}

#[test]
fn malformed_sources_are_parse_failures() {
    assert_eq!(code(render_with("{% if true %}x", &json!({}), None, RenderOptions::default())), codes::PARSE_FAILED);
    assert_eq!(code(render_with("{% endif %}", &json!({}), None, RenderOptions::default())), codes::PARSE_FAILED);
    assert_eq!(
        code(render_with("{% for x in items %}x{% endif %}", &json!({ "items": [] }), None, RenderOptions::default())),
        codes::PARSE_FAILED
    );
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
    assert_eq!(code(render_with("{% set x = 1 %}{{ x }}", &json!({}), None, options)), codes::STEP_BUDGET);

    let options =
        RenderOptions { limits: RenderLimits { max_include_depth: 1, ..RenderLimits::default() }, ..RenderOptions::default() };
    assert_eq!(code(render_with("{% include \"loop.jinja\" %}", &json!({}), None, options)), codes::INCLUDE_DEPTH);
}

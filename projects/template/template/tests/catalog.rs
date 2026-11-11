use template::{
    MapTemplateLoader, TemplateError, TemplateId, TemplateLoader, TemplateRoot, codes, normalize_path, split_scheme,
};

fn loader() -> MapTemplateLoader {
    MapTemplateLoader::new()
        .with_root(TemplateRoot::new("low", 0).with_file("a.hbs", "low").with_file("shared.hbs", "shared"))
        .with_root(
            TemplateRoot::new("high", 10)
                .with_file("a.hbs", "high")
                .with_file("pages/detail.hbs", "detail")
                .with_file("pages/home.hbs", "home")
                .with_file("shared.hbs", "shared-high"),
        )
}

fn error_code(result: Result<TemplateId, TemplateError>) -> String {
    result.map_or_else(|error| error.code().to_string(), |_| String::new())
}

const EXTENSIONS: &[&str] = &["", ".hbs"];

#[test]
fn bare_reference_prefers_higher_priority_root() {
    let id = loader().resolve("a", None, EXTENSIONS).expect("resolve");
    assert_eq!(id.root(), "high");
    assert_eq!(id.path(), "a.hbs");
    assert_eq!(id.to_string(), "high:a.hbs");
}

#[test]
fn scheme_reference_sees_only_named_root() {
    let id = loader().resolve("low:a", None, EXTENSIONS).expect("resolve");
    assert_eq!(id.root(), "low");
    assert_eq!(id.path(), "a.hbs");
}

#[test]
fn relative_reference_resolves_inside_root() {
    let from = TemplateId::new("high", "pages/home.hbs");
    let id = loader().resolve("./detail", Some(&from), EXTENSIONS).expect("resolve");
    assert_eq!(id.path(), "pages/detail.hbs");
    let id = loader().resolve("../shared", Some(&from), EXTENSIONS).expect("resolve");
    assert_eq!(id.root(), "high");
    assert_eq!(id.path(), "shared.hbs");
}

#[test]
fn empty_reference_is_rejected() {
    assert_eq!(error_code(loader().resolve("   ", None, EXTENSIONS)), codes::INVALID_REFERENCE);
}

#[test]
fn unknown_scheme_is_rejected() {
    assert_eq!(error_code(loader().resolve("nope:a", None, EXTENSIONS)), codes::UNKNOWN_SCHEME);
}

#[test]
fn relative_without_from_is_rejected() {
    assert_eq!(error_code(loader().resolve("./a", None, EXTENSIONS)), codes::RELATIVE_WITHOUT_FROM);
}

#[test]
fn relative_root_escape_is_rejected() {
    let from = TemplateId::new("high", "a.hbs");
    assert_eq!(error_code(loader().resolve("../a", Some(&from), EXTENSIONS)), codes::RELATIVE_ESCAPE_ROOT);
}

#[test]
fn unresolved_reference_reports_not_found() {
    assert_eq!(error_code(loader().resolve("missing", None, EXTENSIONS)), codes::NOT_FOUND);
    assert_eq!(error_code(loader().resolve("high:missing", None, EXTENSIONS)), codes::NOT_FOUND);
}

#[test]
fn source_lookup_uses_canonical_id() {
    let loader = loader();
    let id = TemplateId::new("high", "pages/home.hbs");
    assert_eq!(loader.source(&id).expect("source"), "home");
    assert_eq!(error_code_direct(loader.source(&TemplateId::new("nope", "a.hbs"))), codes::UNKNOWN_SCHEME);
    assert_eq!(error_code_direct(loader.source(&TemplateId::new("high", "missing.hbs"))), codes::NOT_FOUND);
}

fn error_code_direct(result: Result<&str, TemplateError>) -> String {
    result.map_or_else(|error| error.code().to_string(), |_| String::new())
}

fn parse_code(id: &str) -> String {
    TemplateId::parse(id).map_or_else(|error| error.code().to_string(), |_| String::new())
}

#[test]
fn scheme_alias_resolves_to_the_root_name() {
    let loader =
        MapTemplateLoader::new().with_root(TemplateRoot::new("view", 0).with_scheme("tpl").with_file("hello.hbs", "hello"));

    let id = loader.resolve("tpl:hello", None, EXTENSIONS).expect("resolve");
    assert_eq!(id.root(), "view");
    assert_eq!(id.path(), "hello.hbs");
    assert_eq!(id.to_string(), "view:hello.hbs");

    assert_eq!(error_code(loader.resolve("view:hello", None, EXTENSIONS)), codes::UNKNOWN_SCHEME);
}

#[test]
fn canonical_ids_parse_and_reject_non_canonical_forms() {
    let id = TemplateId::parse("high:pages/home.hbs").expect("canonical id parses");
    assert_eq!(id, TemplateId::new("high", "pages/home.hbs"));

    for invalid in [
        "high:/pages/home.hbs",
        "high:pages/./home.hbs",
        "high:pages/../home.hbs",
        "high:",
        "high",
        " high:a.hbs",
        "high:a\\b.hbs",
    ] {
        assert_eq!(parse_code(invalid), codes::INVALID_REFERENCE, "`{invalid}` must be rejected");
    }
}

#[test]
fn public_path_normalization_rejects_parent_segments() {
    assert_eq!(normalize_path(" pages\\home.hbs ").as_deref(), Some("pages/home.hbs"));
    assert_eq!(normalize_path("../a"), None);
    assert_eq!(normalize_path(""), None);
}

#[test]
fn public_scheme_splitter_classifies_reference_heads() {
    assert_eq!(split_scheme("high:pages/home.hbs"), Some(("high", "pages/home.hbs")));
    assert_eq!(split_scheme("a-b+c_d:hello"), Some(("a-b+c_d", "hello")));
    assert_eq!(split_scheme("pages/home.hbs"), None);
    assert_eq!(split_scheme("./home.hbs"), None);
    assert_eq!(split_scheme("1bad:hello"), None);
    assert_eq!(split_scheme(":hello"), None);

    // `None` never means an unknown scheme; the loader still reports that.
    let loader = MapTemplateLoader::new().with_root(TemplateRoot::new("view", 0));
    assert_eq!(split_scheme("nope:a"), Some(("nope", "a")));
    assert_eq!(error_code(loader.resolve("nope:a", None, EXTENSIONS)), codes::UNKNOWN_SCHEME);
}

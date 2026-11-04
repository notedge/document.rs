//! 冒烟：fenced code block 之后的分块仍要被结构化，不能退化成原文。

use document_convert::convert_bytes;

const CORPUS: &str = r#"# Fence then structure

```python
def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)
```

## Lists

- First item
- Second item

## Tables

| Header 1 | Header 2 |
|----------|----------|
| Cell 1   | Cell 2   |

## Blockquotes

> This is a simple blockquote.
"#;

#[test]
fn blocks_after_a_fence_keep_their_structure() {
    let output = convert_bytes("markdown", "html", "fence-structure", CORPUS.as_bytes().to_vec(), None).expect("convert");
    let html = output.html.expect("html payload");
    assert!(html.contains("<pre"), "code block survives as a code block: {html}");
    assert!(html.contains("fibonacci(n-1) + fibonacci(n-2)"), "code body is carried verbatim: {html}");
    assert!(html.contains("<h2>Lists</h2>"), "heading after the fence is structured: {html}");
    assert!(html.contains("<h2>Tables</h2>"), "second heading after the fence is structured: {html}");
    assert!(html.contains("<table"), "table after the fence is structured: {html}");
    assert!(!html.contains("| Header 1 |"), "table row is not left as literal text: {html}");
    assert_eq!(output.loss_count, 0, "no silent loss is reported: {html}");
}

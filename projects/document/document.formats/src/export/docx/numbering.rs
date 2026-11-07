use std::fmt::Write as _;

/// Fixed numbering definitions for conservative list export.
pub const NUMBERING_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0">
      <w:numFmt w:val="bullet"/>
    </w:lvl>
    <w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl>
    <w:lvl w:ilvl="2"><w:numFmt w:val="bullet"/></w:lvl>
    <w:lvl w:ilvl="3"><w:numFmt w:val="bullet"/></w:lvl>
    <w:lvl w:ilvl="4"><w:numFmt w:val="bullet"/></w:lvl>
    <w:lvl w:ilvl="5"><w:numFmt w:val="bullet"/></w:lvl>
    <w:lvl w:ilvl="6"><w:numFmt w:val="bullet"/></w:lvl>
    <w:lvl w:ilvl="7"><w:numFmt w:val="bullet"/></w:lvl>
    <w:lvl w:ilvl="8"><w:numFmt w:val="bullet"/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="1">
    <w:abstractNumId w:val="0"/>
  </w:num>
  <w:abstractNum w:abstractNumId="1">
    <w:lvl w:ilvl="0">
      <w:numFmt w:val="decimal"/>
    </w:lvl>
    <w:lvl w:ilvl="1"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="2"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="3"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="4"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="5"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="6"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="7"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="8"><w:numFmt w:val="decimal"/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="2">
    <w:abstractNumId w:val="1"/>
  </w:num>
</w:numbering>"#;

/// Word `numId` for unordered list items.
pub const UNORDERED_NUM_ID: u32 = 1;

/// Word `numId` for ordered list items.
pub const ORDERED_NUM_ID: u32 = 2;

/// One concrete `w:num` that restarts an ordered list at an authored number.
pub const FIRST_DYNAMIC_NUM_ID: u32 = 3;

/// A per-run numbering restart for an ordered list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberingOverride {
    pub num_id: u32,
    pub level: u32,
    pub start: u32,
}

/// Build `word/numbering.xml` with the fixed bullet/decimal abstractions plus
/// one concrete `w:num` per ordered run that must restart at a declared value.
pub fn numbering_xml(overrides: &[NumberingOverride]) -> String {
    let mut xml = String::from(NUMBERING_XML.trim_end_matches("</w:numbering>"));
    for item in overrides {
        let _ = write!(
            xml,
            "  <w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"{}\"><w:startOverride w:val=\"{}\"/></w:lvlOverride></w:num>\n",
            item.num_id, item.level, item.start
        );
    }
    xml.push_str("</w:numbering>");
    xml
}

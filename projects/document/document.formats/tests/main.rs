//! 集成测试入口：按域分子模块。

mod capability;
mod common;
#[cfg(feature = "doc")]
mod doc;
#[cfg(feature = "docx")]
mod docx;
#[cfg(feature = "epub")]
mod epub;
#[cfg(feature = "html")]
mod html;
mod integration;
#[cfg(feature = "markdown")]
mod markdown;
#[cfg(feature = "notedown")]
mod notedown;
#[cfg(feature = "pdf")]
mod pdf;
#[cfg(feature = "markdown")]
mod provenance;
#[cfg(feature = "tex")]
mod tex;
#[cfg(feature = "typst")]
mod typst;

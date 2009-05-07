//! 集成测试入口：按域分子模块。

mod common;
mod convert_report;
mod docx;
mod embed;
mod epub;
mod html;
#[cfg(feature = "logger")]
mod logger;
mod markdown;
mod notedown;
mod pdf;
mod publish;
mod rejection_catalog;
mod route_capabilities;
mod structure_guard;
mod transform;

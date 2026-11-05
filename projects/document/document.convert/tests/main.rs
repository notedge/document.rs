//! 冒烟入口：只保留路由能力的最小 happy path 断言。

mod route_capabilities;

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
mod structure_guard;
mod transform;

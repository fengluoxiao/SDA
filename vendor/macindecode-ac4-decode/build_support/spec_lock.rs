//! 随 crate 分发的规范输入摘要锁。
//!
//! 根目录 `spec/MANIFEST.json` 负责下载；本文件让注册表构建不必信任用户提供的
//! 清单。CI 会校验两处摘要保持一致。

pub(crate) const PART1_TABLES_C_SHA256: &str =
    "5832889afb5828b567447f59c219c820baea5cf864c698833de0f9cc6249d483";
pub(crate) const PART2_TABLES_C_SHA256: &str =
    "2ee628a25e61eeed1ead1fa94cf21eac314e3c2f312e5aac6130763b9926cb7e";
pub(crate) const PDF_TABLES_RS_SHA256: &str =
    "b29bd5fd133d505f9abab4839308f1cd8804dfe3114c7b113ed8397edb5ba32a";

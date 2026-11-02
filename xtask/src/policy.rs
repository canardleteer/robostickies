//! Repository-specific steps, reports, and profiling target.

use crate::CheckStep;

pub(crate) const CHECK_ORDER: [CheckStep; 4] = [
    CheckStep::Fmt,
    CheckStep::Check,
    CheckStep::Clippy,
    CheckStep::Test,
];
pub(crate) const LLVM_COV_REPORT: &str = "target/coverage/llvm-cov/html/index.html";
pub(crate) const TARPAULIN_REPORT: &str = "target/coverage/tarpaulin/tarpaulin-report.html";
pub(crate) const PROFILE_REPORT: &str = "target/profile/profile.json";
pub(crate) const PROFILE_PACKAGE: &str = "robostickies";
pub(crate) const PROFILE_EXAMPLE: &str = "profile_notes";

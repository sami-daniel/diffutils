pub mod cmp;
pub mod context_diff;
pub mod ed_diff;
pub mod macros;
pub mod normal_diff;
pub mod params;
pub mod unified_diff;
pub mod utils;
pub mod side_diff;

// Re-export the public functions/types you need
pub use context_diff::diff as context_diff;
pub use ed_diff::diff as ed_diff;
pub use normal_diff::diff as normal_diff;
pub use unified_diff::diff as unified_diff;
pub use side_diff::diff as side_by_syde_diff;
// Cargo features are package-wide, not selected by the executable's name.
// required-features makes this target available only with the wide backend.
// A distinct source path avoids Cargo's duplicate-source-target warning.
include!("../main.rs");

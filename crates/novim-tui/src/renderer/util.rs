//! Utility functions for line wrapping and layout calculations.

// Re-export shared text utilities so existing callsites within the renderer still compile.
pub(super) use novim_core::text_utils::{wrapped_row_count, wrap_line};

/// Tab colors -- each workspace gets a unique accent color.
pub(super) const TAB_COLORS: &[u8] = &[
    75,  // blue
    114, // green
    176, // purple
    174, // salmon
    180, // gold
    117, // teal
    210, // coral
    149, // lime
    139, // mauve
];

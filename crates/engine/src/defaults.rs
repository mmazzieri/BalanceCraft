//! The areas of a new document: Cerqua's default areas, with the name in the
//! language of the interface.

/// Colour and weight of the default areas, in order: work, family, health,
/// hobbies, chores. Cerqua's `DEFAULT_AREA_SEEDS`.
pub const DEFAULT_AREAS: [(&str, f64); 5] =
    [("#3D6B9C", 30.0), ("#C8553D", 25.0), ("#4A7C59", 20.0), ("#8B5E9C", 15.0), ("#C6882A", 10.0)];

/// Colours offered for an area, as in Cerqua.
pub const AREA_COLORS: [&str; 8] =
    ["#C8553D", "#4A7C59", "#3D6B9C", "#8B5E9C", "#C6882A", "#2E8B8B", "#8B3D3D", "#5A7A3A"];

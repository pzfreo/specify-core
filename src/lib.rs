//! Interview-driven AP242 PMI authoring for STEP parts: a Rust port of specify-core.
//!
//! The modules follow specify-core's (`docs/design.md` maps each Python module to its Rust
//! module). Geometry and STEP come from haecceity, features from quiddity; everything here is
//! deterministic rules and standards tables.

pub mod anchors;
pub mod api;
pub mod choices;
pub mod existing;
pub mod extras;
pub mod features;
pub mod general;
pub mod locations;
pub mod materials;
pub mod mates;
pub mod mesh;
pub mod part;
pub mod problems;
pub mod pyjson;
pub mod resume;
pub mod rules;
pub mod serve;
pub mod sightlines;
pub mod standards;
pub mod triangulate;
pub mod writer;

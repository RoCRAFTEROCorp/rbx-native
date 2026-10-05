//! The Terrain Editor's edits, as plain functions over a [`VoxelGrid`].
//!
//! [`brush`] and [`stroke`] are the brush tools (Draw, Sculpt, Smooth,
//! Flatten, Paint); [`region`] the tools that act on a selected box (Fill,
//! Replace, Sea Level, Delete); [`clip`] copy, paste and Transform.
//!
//! [`VoxelGrid`]: crate::VoxelGrid

pub mod brush;
pub mod clip;
pub mod region;
pub mod stroke;

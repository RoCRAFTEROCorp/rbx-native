//! Roblox smooth terrain: the voxel grid behind `Terrain.SmoothGrid`, the
//! codecs for it and the properties saved beside it (`PhysicsGrid`,
//! `MaterialColors`), and the edits the Terrain Editor makes to it.
//!
//! Entry points: [`smooth_grid::decode`]/[`smooth_grid::encode`] to read and
//! write the voxels, [`VoxelGrid`] to hold and change them.

mod cell;
pub mod edit;
pub mod generate;
mod grid;
pub mod import;
mod material;
mod material_colors;
pub mod mesh;
mod noise;
pub mod physics_grid;
mod raycast;
pub mod smooth_grid;

pub use cell::{quantize, Cell, FULL};
pub use grid::{ChunkKey, VoxelGrid, CHUNK, VOXEL_STUDS};
pub use material::Material;
pub use material_colors::MaterialColors;
pub use raycast::{raycast, Hit};
pub use smooth_grid::SmoothGridError;

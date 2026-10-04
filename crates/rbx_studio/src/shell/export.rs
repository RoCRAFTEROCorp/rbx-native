//! The Explorer row menu's Save / Export rows, named as Studio's own row menu
//! names them (`creator-docs`): **Save to File…** writes the selection as a
//! model file (`.rbxm`, or `.rbxmx` when that is the name picked), **Export
//! Selection…** as a Wavefront `.obj`, and **Export as glTF…** as `.gltf`.
//! The meshes are what the viewport draws (see `rbx_viewer::export`).
//!
//! `RBX_STUDIO_EXPORT_DIR=<dir>` skips the save dialog and writes
//! `<dir>/<name>.<extension>` — a debugging aid for driving an export
//! headlessly, where no file chooser portal may answer.

use std::path::{Path, PathBuf};

use gpui_kit::Context;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::{export, pick};

use crate::command_bar::Feedback;
use crate::save::{write_atomic, Format};

use super::{clipboard, Shell};

const EXPORT_DIR_VARIABLE: &str = "RBX_STUDIO_EXPORT_DIR";
/// The `source` an export's row carries in the Output dock.
const SOURCE: &str = "Export";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Export {
    Model,
    Obj,
    Gltf,
}

impl Export {
    fn extension(self) -> &'static str {
        match self {
            Export::Model => "rbxm",
            Export::Obj => "obj",
            Export::Gltf => "gltf",
        }
    }
}

/// Whether the mesh exports have anything to write: some part in, or
/// being, the selection.
pub(super) fn has_geometry(dom: &WeakDom, database: &ReflectionDatabase, selected: &[Ref]) -> bool {
    selected
        .iter()
        .any(|&root| pick::parts_of(dom, database, root).next().is_some())
}

/// Each file to write and its bytes, `path` first. A model goes out in the
/// format its extension names, binary unless that is `.rbxmx`; an `.obj`
/// brings its `.mtl` and any part's texture `.png`s along beside it.
fn encode(
    kind: Export,
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &pick::Meshes,
    roots: &[Ref],
    path: &Path,
) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let solids = || {
        let solids = export::meshes_of(dom, database, meshes, roots);
        match solids.meshes.is_empty() {
            true => Err("nothing to export: the selection holds no parts".to_string()),
            false => Ok(solids),
        }
    };
    match kind {
        Export::Model => {
            let xml = path.extension().is_some_and(|ext| ext == "rbxmx");
            let format = if xml { Format::Xml } else { Format::Binary };
            let bytes = format.encode(&clipboard::detached(dom, roots))?;
            Ok(vec![(path.to_path_buf(), bytes)])
        }
        Export::Obj => {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            Ok(export::obj_files(&solids()?, &stem)
                .into_iter()
                .map(|(name, bytes)| (path.with_file_name(name), bytes))
                .collect())
        }
        Export::Gltf => Ok(vec![(
            path.to_path_buf(),
            export::gltf(&solids()?).into_bytes(),
        )]),
    }
}

impl Shell {
    /// Asks where to write the selection, then writes it there.
    pub(super) fn export_selected(&mut self, kind: Export, cx: &mut Context<Self>) {
        let roots = self.selected_all().to_vec();
        let Some(name) = roots
            .first()
            .and_then(|&root| self.dom.get(root))
            .map(|instance| instance.name().to_owned())
        else {
            return;
        };
        let suggested = format!("{name}.{}", kind.extension());
        if let Ok(directory) = std::env::var(EXPORT_DIR_VARIABLE) {
            self.export_to(kind, &roots, &Path::new(&directory).join(suggested), cx);
            return;
        }
        let directory = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let picked = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn(async move |this, cx| {
            let Ok(picked) = picked.await else {
                return;
            };
            let _ = this.update(cx, |shell, cx| match picked {
                Ok(Some(path)) => shell.export_to(kind, &roots, &path, cx),
                Ok(None) => {}
                // No file chooser portal to ask: say so rather than a menu
                // row that silently does nothing.
                Err(err) => shell.report_export(Err(format!("no save dialog: {err:#}")), cx),
            });
        })
        .detach();
    }

    fn export_to(&mut self, kind: Export, roots: &[Ref], path: &Path, cx: &mut Context<Self>) {
        // An open script's text reaches the DOM on a debounce; a model file
        // has to hold what is on screen, as a save does.
        self.flush_script_edits(cx);
        let meshes = self.viewport.read(cx).meshes().clone();
        let result = encode(kind, &self.dom, &self.database, &meshes, roots, path)
            .and_then(|files| {
                files
                    .iter()
                    .try_for_each(|(path, bytes)| write_atomic(path, bytes))
            })
            .map(|()| format!("Exported {}", path.display()));
        self.report_export(result, cx);
    }

    fn report_export(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        let feedback = match result {
            Ok(message) => Feedback::Output(message),
            Err(message) => Feedback::Error(message),
        };
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }
}

#[cfg(test)]
mod tests;

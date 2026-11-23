//! Every host function a script can be given, with the capability it needs. A run's host
//! table holds only the ones its capabilities grant.

use crate::binding::Binding;
use crate::caps::{Capabilities, Capability};
use crate::values::Args;
use crate::{edits, files, reads};
use pikslar_script::{HostCx, HostError, HostTable, TableError, Value};

pub type Run = fn(&mut Binding<'_, '_>, &Args<'_>, &mut HostCx<'_, '_>) -> Result<Value, HostError>;

#[derive(Clone, Copy)]
pub struct Func {
    pub name: &'static str,
    pub arity: usize,
    /// None for a function every run gets.
    pub needs: Option<Capability>,
    pub run: Run,
}

/// A host function as the documentation lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionInfo {
    pub name: &'static str,
    pub arity: usize,
    pub needs: Option<Capability>,
}

const fn f(name: &'static str, arity: usize, needs: Option<Capability>, run: Run) -> Func {
    Func {
        name,
        arity,
        needs,
        run,
    }
}

const R: Option<Capability> = Some(Capability::ReadDocument);
const E: Option<Capability> = Some(Capability::EditDocument);
const P: Option<Capability> = Some(Capability::PickFileToRead);
const W: Option<Capability> = Some(Capability::PickFileToWrite);

const ALL: &[Func] = &[
    f("print", 1, None, reads::print),
    f("api_version", 0, R, reads::api_version),
    f("width", 0, R, reads::width),
    f("height", 0, R, reads::height),
    f("layers", 0, R, reads::layers),
    f("frames", 0, R, reads::frames),
    f("layer_name", 1, R, reads::layer_name),
    f("layer_kind", 1, R, reads::layer_kind),
    f("layer_index", 1, R, reads::layer_index),
    f("layer_visible", 1, R, reads::layer_visible),
    f("layer_locked", 1, R, reads::layer_locked),
    f("layer_opacity", 1, R, reads::layer_opacity),
    f("layer_blend", 1, R, reads::layer_blend),
    f("frame_duration", 1, R, reads::frame_duration),
    f("get_pixel", 4, R, reads::get_pixel),
    f("read_rect", 6, R, reads::read_rect),
    f("swatches", 0, R, reads::swatches),
    f("swatch_color", 1, R, reads::swatch_color),
    f("text_width", 2, R, reads::text_width),
    f("add_layer", 1, E, edits::add_layer),
    f("add_layer_above", 2, E, edits::add_layer_above),
    f("remove_layer", 1, E, edits::remove_layer),
    f("rename_layer", 2, E, edits::rename_layer),
    f("move_layer", 2, E, edits::move_layer),
    f("duplicate_layer", 1, E, edits::duplicate_layer),
    f("set_opacity", 2, E, edits::set_opacity),
    f("set_visible", 2, E, edits::set_visible),
    f("set_blend", 2, E, edits::set_blend),
    f("add_frame", 1, E, edits::add_frame),
    f("copy_frame", 1, E, edits::copy_frame),
    f("remove_frame", 1, E, edits::remove_frame),
    f("set_duration", 2, E, edits::set_duration),
    f("add_tag", 3, E, edits::add_tag),
    f("set_pixel", 5, E, edits::set_pixel),
    f("erase_pixel", 4, E, edits::erase_pixel),
    f("draw_line", 7, E, edits::draw_line),
    f("fill", 5, E, edits::fill),
    f("fill_rect", 7, E, edits::fill_rect),
    f("write_rect", 6, E, edits::write_rect),
    f("draw_text", 7, E, edits::draw_text),
    f("add_swatch", 1, E, edits::add_swatch),
    f("recolor", 2, E, edits::recolor),
    f("remove_swatch", 1, E, edits::remove_swatch),
    f("pick_file_to_read", 2, P, files::pick_file_to_read),
    f("read_file", 1, P, files::read_file),
    f("file_name", 1, P, files::file_name),
    f("pick_file_to_write", 2, W, files::pick_file_to_write),
    f("write_file", 2, W, files::write_file),
];

/// Every host function there is, in the order the documentation lists them.
pub fn functions() -> Vec<FunctionInfo> {
    ALL.iter()
        .map(|f| FunctionInfo {
            name: f.name,
            arity: f.arity,
            needs: f.needs,
        })
        .collect()
}

/// The host table for one run, and what each id in it runs: only what `caps` grants.
pub fn granted(caps: Capabilities) -> Result<(HostTable, Vec<Func>), TableError> {
    let mut table = HostTable::new();
    let mut fns = Vec::new();
    for func in ALL.iter().filter(|f| f.needs.is_none_or(|c| caps.has(c))) {
        table.add(func.name, func.arity)?;
        fns.push(*func);
    }
    Ok((table, fns))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_function_has_a_distinct_valid_name() {
        let every = Capabilities::ALL
            .with(Capability::PickFileToRead)
            .with(Capability::PickFileToWrite);
        let (_, all) = granted(every).expect("a valid table");
        assert_eq!(all.len(), ALL.len());
    }

    #[test]
    fn each_capability_alone_grants_exactly_its_own_functions() {
        let each = [
            Capability::ReadDocument,
            Capability::EditDocument,
            Capability::PickFileToRead,
            Capability::PickFileToWrite,
        ];
        for cap in each {
            let (_, fns) = granted(Capabilities::NONE.with(cap)).expect("a table");
            let wanted = ALL
                .iter()
                .filter(|f| f.needs.is_none() || f.needs == Some(cap));
            let wanted: Vec<_> = wanted.map(|f| f.name).collect();
            assert_eq!(
                fns.iter().map(|f| f.name).collect::<Vec<_>>(),
                wanted,
                "{cap:?}"
            );
        }
    }

    #[test]
    fn a_run_gets_only_what_it_is_granted() {
        let (_, none) = granted(Capabilities::NONE).expect("a table");
        assert_eq!(none.iter().map(|f| f.name).collect::<Vec<_>>(), ["print"]);
        let (_, read) = granted(Capabilities::READ).expect("a table");
        assert!(read.iter().all(|f| f.needs != E));
        assert!(read.iter().any(|f| f.name == "get_pixel"));
        let edit = Capabilities::NONE.with(Capability::EditDocument);
        let (_, edit) = granted(edit).expect("a table");
        assert!(edit.iter().all(|f| f.needs != R));
        assert!(edit.iter().any(|f| f.name == "set_pixel"));
    }
}

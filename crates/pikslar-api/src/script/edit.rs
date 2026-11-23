//! Layers, frames, tags and swatches: each operation checked whole, then one or two commands.

use super::Script;
use crate::{input, ApiError, FrameRef, LayerRef, Mode, Op, Reply};
use pikslar_core::command::MAX_LAYER_NAME;
use pikslar_core::{ColorMode, FrameCmd, LayerCmd, LimitError, PaletteCmd, TagCmd, MAX_TAG_NAME};

fn color_mode(mode: Mode) -> ColorMode {
    match mode {
        Mode::Rgba => ColorMode::Rgba,
        Mode::Palette => ColorMode::Palette,
    }
}

impl Script<'_> {
    pub(super) fn edit_layer(&mut self, op: Op) -> Result<Reply, ApiError> {
        match op {
            Op::AddLayer { above, mode, name } => return self.add_layer(above, mode, name),
            Op::DuplicateLayer { layer } => {
                let layer = input::layer_id(self.doc(), layer)?;
                self.apply(LayerCmd::Duplicate { layer })?;
                return self.active_layer_id();
            }
            Op::RemoveLayer { layer } => {
                let layer = input::layer_id(self.doc(), layer)?;
                self.apply(LayerCmd::Remove { layer })?;
            }
            Op::RenameLayer { layer, name } => {
                let layer = input::layer_id(self.doc(), layer)?;
                let name = input::name(&name, MAX_LAYER_NAME)?;
                self.apply(LayerCmd::Rename { layer, name })?;
            }
            Op::MoveLayer { layer, to } => {
                let layer = input::layer_id(self.doc(), layer)?;
                let len = self.doc().layers().len();
                let to = input::place("a layer's place", to, len, false)?;
                self.apply(LayerCmd::Move { layer, to })?;
            }
            Op::SetOpacity { layer, opacity } => {
                let layer = input::layer_id(self.doc(), layer)?;
                let opacity = input::opacity(opacity)?;
                self.apply_ended(LayerCmd::SetOpacity { layer, opacity })?;
            }
            Op::SetVisible { layer, visible } => {
                let layer = input::layer_id(self.doc(), layer)?;
                self.apply(LayerCmd::SetVisible { layer, visible })?;
            }
            Op::SetBlend { layer, blend } => {
                let layer = input::layer_id(self.doc(), layer)?;
                let blend = input::blend(self.doc(), &blend)?;
                self.apply(LayerCmd::SetBlend { layer, blend })?;
            }
            other => return self.call(other),
        }
        Ok(Reply::Done)
    }

    fn add_layer(
        &mut self,
        above: Option<LayerRef>,
        mode: Mode,
        name: Option<String>,
    ) -> Result<Reply, ApiError> {
        let at = match above {
            Some(layer) => input::layer_index(self.doc(), layer)? + 1,
            None => self.doc().layers().len(),
        };
        let name = name
            .map(|name| input::name(&name, MAX_LAYER_NAME))
            .transpose()?;
        let mode = color_mode(mode);
        self.apply(LayerCmd::Add { at, mode })?;
        if let Some(name) = name {
            let layer = self.editor.layer_id(self.editor.layer);
            let layer = layer.ok_or(ApiError::Refused(LimitError::NoSuchItem))?;
            self.apply(LayerCmd::Rename { layer, name })?;
        }
        self.active_layer_id()
    }

    pub(super) fn edit_timeline_or_palette(&mut self, op: Op) -> Result<Reply, ApiError> {
        match op {
            Op::AddFrame { at, copy } => {
                let len = self.doc().frames.len();
                let at = input::place("a new frame's place", at, len, true)?;
                self.apply(FrameCmd::Add { at, copy })?;
                let made = self.doc().frames.get(at).map(|f| f.key().get());
                return made
                    .map(Reply::Frame)
                    .ok_or(ApiError::Refused(LimitError::NoSuchItem));
            }
            Op::RemoveFrame { frame } => {
                let at = input::frame(self.doc(), frame)?;
                self.apply(FrameCmd::Remove { from: at, to: at })?;
            }
            Op::SetDuration { frame, ms } => {
                let from = input::frame(self.doc(), frame)?;
                let durations = vec![input::duration(ms)?];
                self.apply_ended(FrameCmd::Retime { from, durations })?;
            }
            Op::AddTag { name, from, to } => self.add_tag(&name, from, to)?,
            Op::AddSwatch { color } => {
                let color = input::color(color)?;
                let made = self.apply(PaletteCmd::AddSwatch { color })?.created;
                made.into_iter().for_each(|id| self.saw_color(id));
                return made
                    .map(Reply::Created)
                    .ok_or(ApiError::Refused(LimitError::NoSuchItem));
            }
            Op::Recolor { swatch, color } => {
                let id = input::swatch(self.doc(), swatch)?;
                let color = input::color(color)?;
                self.apply_ended(PaletteCmd::Recolor { id, color })?;
            }
            Op::RemoveSwatch { swatch } => {
                let id = input::swatch(self.doc(), swatch)?;
                self.apply(PaletteCmd::RemoveSwatch { id })?;
            }
            other => return self.call(other),
        }
        Ok(Reply::Done)
    }

    fn add_tag(&mut self, name: &str, from: FrameRef, to: FrameRef) -> Result<(), ApiError> {
        let name = input::name(name, MAX_TAG_NAME)?;
        let first = input::frame(self.doc(), from)?;
        let last = input::frame(self.doc(), to)?;
        if last < first {
            let place = |at: usize| i64::try_from(at).unwrap_or(i64::MAX);
            let end = place(self.doc().frames.len().saturating_sub(1));
            input::ranged("a tag's last frame's place", place(last), place(first), end)?;
        }
        self.apply_ended(TagCmd::Add {
            name,
            from: first,
            to: last,
        })
    }
}

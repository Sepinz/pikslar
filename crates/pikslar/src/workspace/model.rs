//! Panels, dock areas, slots, presets and the saved arrangement of each preset (section 3.1 to
//! 3.6 of docs/ui-redesign.md). Every type is `Copy` and fixed in size apart from the custom
//! workspace's name, so an arrangement can be solved every frame without allocating.

/// How many slots a dock area holds.
pub(crate) const MOST_SLOTS: usize = 3;
/// How many panels there are, and so the most tabs one slot can hold.
pub(crate) const PANEL_COUNT: usize = 10;
/// A slot's tab strip at its least, which is all of a folded slot (units). The app draws with
/// `layout::strip()`, which grows with the face; this is the solver tests' default, kept equal to
/// `tabs::STRIP_H` by a test.
pub(crate) const TAB_STRIP: i32 = 20;
/// A folded dock area: one strip of panel icons (units).
pub(crate) const RAIL: i32 = 24;
/// The Color panel's compact form: swatch, value, unfold (units).
pub(crate) const COLOR_COMPACT: i32 = 56;
/// The timeline's layer column: least, default and largest width (units).
pub(crate) const NAMES_RANGE: (i32, i32) = (120, 320);
pub(crate) const NAMES_DEFAULT: i32 = 176;
/// The largest a stored slot size may be; larger is clamped (units).
pub(crate) const MOST_SLOT_SIZE: i32 = 2048;

/// One of the ten panels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Panel {
    Color,
    Palette,
    Layers,
    Timeline,
    History,
    Tiles,
    Preview,
    Lint,
    Slices,
    Scripts,
}

impl Panel {
    pub(crate) const ALL: [Panel; PANEL_COUNT] = [
        Panel::Color,
        Panel::Palette,
        Panel::Layers,
        Panel::Timeline,
        Panel::History,
        Panel::Tiles,
        Panel::Preview,
        Panel::Lint,
        Panel::Slices,
        Panel::Scripts,
    ];

    /// The name used in the settings file.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Panel::Color => "color",
            Panel::Palette => "palette",
            Panel::Layers => "layers",
            Panel::Timeline => "timeline",
            Panel::History => "history",
            Panel::Tiles => "tiles",
            Panel::Preview => "preview",
            Panel::Lint => "lint",
            Panel::Slices => "slices",
            Panel::Scripts => "scripts",
        }
    }

    pub(crate) fn from_word(word: &str) -> Option<Panel> {
        Panel::ALL.into_iter().find(|p| p.word() == word)
    }

    /// The least height of a slot showing this panel in a side dock, tab strip included
    /// (table 3.2), with Color in its full form.
    pub(crate) fn least(self) -> i32 {
        match self {
            Panel::Color => 150,
            Panel::Palette => 64,
            Panel::Layers => 110,
            Panel::Timeline | Panel::History => 28,
            Panel::Tiles => 120,
            Panel::Preview | Panel::Scripts => 80,
            Panel::Lint | Panel::Slices => 60,
        }
    }

    /// The least width of a slot showing this panel side by side in the bottom dock. Section
    /// 3.2 gives heights only; these widths are this module's choice.
    pub(crate) fn least_across(self) -> i32 {
        match self {
            Panel::Timeline => 240,
            _ => 160,
        }
    }

    /// Color and Timeline are never hidden: they only compact or fold.
    pub(crate) fn can_hide(self) -> bool {
        !matches!(self, Panel::Color | Panel::Timeline)
    }

    /// The order in which the solver folds slots that do not fit, lowest first (section 3.8);
    /// `None` for panels that never fold away. Scripts is not in the documented list; it folds
    /// after Tiles so that a script's output stays visible longest.
    pub(crate) fn fold_rank(self) -> Option<u8> {
        match self {
            Panel::Lint => Some(0),
            Panel::Slices => Some(1),
            Panel::Preview => Some(2),
            Panel::Palette => Some(3),
            Panel::History => Some(4),
            Panel::Tiles => Some(5),
            Panel::Scripts => Some(6),
            Panel::Color | Panel::Layers | Panel::Timeline => None,
        }
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// A set of panels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PanelSet(u16);

impl PanelSet {
    pub(crate) const EMPTY: PanelSet = PanelSet(0);

    pub(crate) fn contains(self, p: Panel) -> bool {
        self.0 & p.bit() != 0
    }

    pub(crate) fn with(self, p: Panel) -> PanelSet {
        PanelSet(self.0 | p.bit())
    }

    pub(crate) fn without(self, p: Panel) -> PanelSet {
        PanelSet(self.0 & !p.bit())
    }

    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = Panel> {
        Panel::ALL.into_iter().filter(move |&p| self.contains(p))
    }
}

/// The panels of one slot, in tab order, without repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tabs {
    items: [Panel; PANEL_COUNT],
    len: u8,
}

impl Default for Tabs {
    fn default() -> Self {
        Tabs {
            items: [Panel::Color; PANEL_COUNT],
            len: 0,
        }
    }
}

impl Tabs {
    /// The panels in order; repeats after the first are left out.
    pub(crate) fn of(panels: &[Panel]) -> Tabs {
        panels.iter().fold(Tabs::default(), |t, &p| t.with(p))
    }

    pub(crate) fn as_slice(&self) -> &[Panel] {
        &self.items[..usize::from(self.len)]
    }

    pub(crate) fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn contains(&self, p: Panel) -> bool {
        self.as_slice().contains(&p)
    }

    /// With `p` added last, unless it is there already.
    pub(crate) fn with(self, p: Panel) -> Tabs {
        if self.contains(p) || self.len() >= PANEL_COUNT {
            return self;
        }
        let mut items = self.items;
        items[self.len()] = p;
        Tabs {
            items,
            len: self.len + 1,
        }
    }

    /// Without `p`, the others keeping their order.
    pub(crate) fn without(self, p: Panel) -> Tabs {
        Tabs::of(
            &self
                .as_slice()
                .iter()
                .copied()
                .filter(|&q| q != p)
                .collect::<Vec<_>>(),
        )
    }
}

/// A stack position in a dock area: a tab strip and the body of its active panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Slot {
    /// Along the area's stacking direction, in units; 0 takes the rest.
    pub size: i32,
    /// False when folded to its tab strip.
    pub open: bool,
    /// Index of the active tab.
    pub active: u8,
    pub tabs: Tabs,
}

impl Slot {
    pub(crate) fn new(size: i32, panels: &[Panel]) -> Slot {
        Slot {
            size,
            open: true,
            active: 0,
            tabs: Tabs::of(panels),
        }
    }

    pub(crate) fn active_panel(&self) -> Option<Panel> {
        let tabs = self.tabs.as_slice();
        tabs.get(usize::from(self.active)).or(tabs.last()).copied()
    }

    /// Whether it holds the Timeline alone, which draws no tab strip and so has nothing to
    /// unfold it by: such a slot never folds.
    pub(crate) fn timeline_alone(&self) -> bool {
        self.tabs.as_slice() == [Panel::Timeline]
    }

    /// The same slot, opened if it holds the Timeline alone.
    pub(crate) fn reachable(self) -> Slot {
        Slot {
            open: self.open || self.timeline_alone(),
            ..self
        }
    }
}

/// The three dock areas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Side {
    Right,
    Bottom,
    Left,
}

impl Side {
    /// Reading order: where a panel listed twice keeps its first place.
    pub(crate) const ALL: [Side; 3] = [Side::Right, Side::Bottom, Side::Left];

    pub(crate) fn word(self) -> &'static str {
        match self {
            Side::Right => "right",
            Side::Bottom => "bottom",
            Side::Left => "left",
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }

    /// Least and largest stored size in units. The bottom dock's largest is also held to half
    /// the window by the solver.
    pub(crate) fn range(self) -> (i32, i32) {
        match self {
            Side::Right => (200, 400),
            Side::Left => (200, 320),
            Side::Bottom => (28, 1024),
        }
    }
}

/// One dock area: its size across the window edge and up to three slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Area {
    /// Width for the side docks, height for the bottom dock, in units.
    pub size: i32,
    /// False when folded to its rail.
    pub open: bool,
    slots: [Slot; MOST_SLOTS],
    count: u8,
}

impl Area {
    pub(crate) fn new(size: i32, open: bool, slots: &[Slot]) -> Area {
        let mut a = Area {
            size,
            open,
            ..Area::default()
        };
        for s in slots.iter().take(MOST_SLOTS) {
            a.slots[a.slots().len()] = *s;
            a.count += 1;
        }
        a
    }

    pub(crate) fn slots(&self) -> &[Slot] {
        &self.slots[..usize::from(self.count)]
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The same area with slot `i` replaced; out of range changes nothing.
    pub(crate) fn with_slot(self, i: usize, slot: Slot) -> Area {
        let mut a = self;
        if i < a.slots().len() {
            a.slots[i] = slot;
        }
        a
    }

    /// The same area at another size.
    pub(crate) fn with_size(self, size: i32) -> Area {
        Area { size, ..self }
    }

    /// The same area opened or folded to its rail.
    pub(crate) fn with_open(self, open: bool) -> Area {
        Area { open, ..self }
    }
}

/// The arrangement of one preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Workspace {
    areas: [Area; 3],
    /// Color in its compact form wherever it is.
    pub color_compact: bool,
    /// Panels hidden by hand: not placed, and not put back by the repair rules.
    pub hidden: PanelSet,
}

impl Workspace {
    pub(crate) fn new(areas: [Area; 3], color_compact: bool, hidden: PanelSet) -> Workspace {
        Workspace {
            areas,
            color_compact,
            hidden,
        }
    }

    pub(crate) fn with_compact(self, color_compact: bool) -> Workspace {
        Workspace {
            color_compact,
            ..self
        }
    }

    pub(crate) fn with_hidden(self, hidden: PanelSet) -> Workspace {
        Workspace { hidden, ..self }
    }

    pub(crate) fn area(&self, side: Side) -> &Area {
        &self.areas[side.index()]
    }

    pub(crate) fn with_area(self, side: Side, area: Area) -> Workspace {
        let mut w = self;
        w.areas[side.index()] = area;
        w
    }

    /// Every panel placed in some slot.
    pub(crate) fn placed(&self) -> PanelSet {
        self.areas
            .iter()
            .flat_map(|a| a.slots().iter())
            .flat_map(|s| s.tabs.as_slice().iter())
            .fold(PanelSet::EMPTY, |set, &p| set.with(p))
    }

    /// Where `p` is: area, slot and tab index.
    pub(crate) fn find(&self, p: Panel) -> Option<(Side, usize, usize)> {
        Side::ALL.into_iter().find_map(|side| {
            self.area(side)
                .slots()
                .iter()
                .enumerate()
                .find_map(|(i, s)| {
                    s.tabs
                        .as_slice()
                        .iter()
                        .position(|&q| q == p)
                        .map(|t| (side, i, t))
                })
        })
    }
}

/// The four built-in presets and the one custom workspace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Preset {
    #[default]
    Draw,
    Animate,
    Tiles,
    Palette,
    Custom,
}

impl Preset {
    pub(crate) const BUILT_IN: [Preset; 4] = [
        Preset::Draw,
        Preset::Animate,
        Preset::Tiles,
        Preset::Palette,
    ];

    pub(crate) fn word(self) -> &'static str {
        match self {
            Preset::Draw => "draw",
            Preset::Animate => "animate",
            Preset::Tiles => "tiles",
            Preset::Palette => "palette",
            Preset::Custom => "custom",
        }
    }

    pub(crate) fn from_word(word: &str) -> Option<Preset> {
        [Preset::Custom]
            .into_iter()
            .chain(Preset::BUILT_IN)
            .find(|p| p.word() == word)
    }
}

/// The one custom workspace, saved by the user under a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Custom {
    pub name: String,
    pub layout: Workspace,
}

/// Every workspace kept between runs: the preset in use, each built-in preset's arrangement
/// (changes are kept per preset), the custom workspace and the timeline's layer column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Workspaces {
    pub current: Preset,
    built_in: [Workspace; 4],
    pub custom: Option<Custom>,
    pub timeline_names: i32,
}

impl Default for Workspaces {
    fn default() -> Self {
        Workspaces {
            current: Preset::Draw,
            built_in: Preset::BUILT_IN.map(crate::workspace::presets::default_layout),
            custom: None,
            timeline_names: NAMES_DEFAULT,
        }
    }
}

impl Workspaces {
    /// The arrangement of `preset`, or none for a custom workspace never saved.
    pub(crate) fn layout(&self, preset: Preset) -> Option<Workspace> {
        match preset {
            Preset::Custom => self.custom.as_ref().map(|c| c.layout),
            p => Some(self.built_in[p as usize]),
        }
    }

    /// The arrangement in use.
    pub(crate) fn active(&self) -> Workspace {
        self.layout(self.current)
            .unwrap_or_else(|| self.built_in[Preset::Draw as usize])
    }

    /// Switches preset; a custom workspace never saved leaves things as they are.
    pub(crate) fn choose(&self, preset: Preset) -> Workspaces {
        match self.layout(preset) {
            Some(_) => Workspaces {
                current: preset,
                ..self.clone()
            },
            None => self.clone(),
        }
    }

    /// With `preset` arranged as `layout`. A custom workspace never saved stays unsaved.
    pub(crate) fn with_layout(&self, preset: Preset, layout: Workspace) -> Workspaces {
        let mut w = self.clone();
        match preset {
            Preset::Custom => {
                if let Some(c) = w.custom.as_mut() {
                    c.layout = layout;
                }
            }
            p => w.built_in[p as usize] = layout,
        }
        w
    }

    /// Window > Workspace > Reset: the preset's default arrangement. The custom workspace has
    /// none, so resetting it changes nothing.
    pub(crate) fn reset(&self, preset: Preset) -> Workspaces {
        match preset {
            Preset::Custom => self.clone(),
            p => self.with_layout(p, crate::workspace::presets::default_layout(p)),
        }
    }

    /// Window > Workspace > Save as custom: the arrangement in use, under `name`, replacing
    /// the one custom workspace, which becomes the one in use.
    pub(crate) fn save_custom(&self, name: &str) -> Workspaces {
        Workspaces {
            current: Preset::Custom,
            custom: Some(Custom {
                name: clean_name(name),
                layout: self.active(),
            }),
            ..self.clone()
        }
    }

    pub(crate) fn with_names(&self, width: i32) -> Workspaces {
        Workspaces {
            timeline_names: width.clamp(NAMES_RANGE.0, NAMES_RANGE.1),
            ..self.clone()
        }
    }
}

/// The most characters a custom workspace's name keeps.
pub(crate) const MOST_NAME_CHARS: usize = 32;

/// A custom workspace's name as kept: no control characters, trimmed, at most
/// [`MOST_NAME_CHARS`], and "Custom" when nothing is left.
pub(crate) fn clean_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MOST_NAME_CHARS)
        .collect();
    match kept.trim() {
        "" => "Custom".to_string(),
        t => t.to_string(),
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

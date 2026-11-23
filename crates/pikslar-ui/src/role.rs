//! What each widget is and what state it is in, beside its name: the structure a screen reader
//! would need later (docs/ui-redesign.md 3.12), listed by the layout dump in columns 7 and 8.

/// What kind of control a widget is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    /// Not said: a widget the app draws by hand.
    #[default]
    None,
    Button,
    Toggle,
    Tab,
    TabList,
    Splitter,
    Slider,
    ListItem,
    MenuItem,
    TextField,
    Group,
    /// A number with buttons to step it, such as a stepper: a UI Automation spinner later.
    SpinButton,
}

impl Role {
    /// The word the layout dump writes; empty for [`Role::None`].
    pub fn word(self) -> &'static str {
        match self {
            Role::None => "",
            Role::Button => "button",
            Role::Toggle => "toggle",
            Role::Tab => "tab",
            Role::TabList => "tab-list",
            Role::Splitter => "splitter",
            Role::Slider => "slider",
            Role::ListItem => "list-item",
            Role::MenuItem => "menu-item",
            Role::TextField => "text-field",
            Role::Group => "group",
            Role::SpinButton => "spinbutton",
        }
    }
}

/// The state a widget shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// Chosen: the tool, the tab shown, a toggle that is on.
    pub chosen: bool,
    /// Open or folded, for what can be either.
    pub expanded: Option<bool>,
    pub disabled: bool,
    /// A number it holds: a stepper's value, a slider's per cent, a splitter's size in units.
    pub value: Option<i32>,
}

impl State {
    pub const NONE: State = State {
        chosen: false,
        expanded: None,
        disabled: false,
        value: None,
    };

    /// Chosen when `on`, nothing else.
    pub const fn chosen(on: bool) -> State {
        State {
            chosen: on,
            ..State::NONE
        }
    }

    /// Holding `value`, nothing else.
    pub const fn value(value: i32) -> State {
        State {
            value: Some(value),
            ..State::NONE
        }
    }

    /// The words the layout dump writes, separated by commas: `chosen`, `expanded` or
    /// `collapsed`, `disabled`, `value=N`. Empty when there is nothing to say.
    pub fn words(self) -> String {
        let mut out = String::new();
        let mut put = |word: &str| {
            if !out.is_empty() {
                out.push(',');
            }
            out.push_str(word);
        };
        if self.chosen {
            put("chosen");
        }
        match self.expanded {
            Some(true) => put("expanded"),
            Some(false) => put("collapsed"),
            None => {}
        }
        if self.disabled {
            put("disabled");
        }
        if let Some(v) = self.value {
            put(&format!("value={v}"));
        }
        out
    }
}

/// A widget's role and state together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kind {
    pub role: Role,
    pub state: State,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_but_none_has_its_own_word_without_tabs_or_spaces() {
        let all = [
            Role::Button,
            Role::Toggle,
            Role::Tab,
            Role::TabList,
            Role::Splitter,
            Role::Slider,
            Role::ListItem,
            Role::MenuItem,
            Role::TextField,
            Role::Group,
            Role::SpinButton,
        ];
        let mut words: Vec<&str> = all.iter().map(|r| r.word()).collect();
        assert!(words
            .iter()
            .all(|w| !w.is_empty() && !w.contains([' ', '\t'])));
        words.sort_unstable();
        words.dedup();
        assert_eq!(words.len(), all.len());
        assert_eq!(Role::None.word(), "");
    }

    #[test]
    fn state_words_list_what_is_true_in_a_fixed_order() {
        assert_eq!(State::NONE.words(), "");
        assert_eq!(State::chosen(true).words(), "chosen");
        assert_eq!(State::value(-3).words(), "value=-3");
        let all = State {
            chosen: true,
            expanded: Some(false),
            disabled: true,
            value: Some(12),
        };
        assert_eq!(all.words(), "chosen,collapsed,disabled,value=12");
        let open = State {
            expanded: Some(true),
            ..State::NONE
        };
        assert_eq!(open.words(), "expanded");
    }
}

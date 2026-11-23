//! How a layer mixes with the layers below it, and how a document combines its layers.

/// How a layer mixes with the layers below it. The order is the one files store and menus show.
///
/// The first nine are Pikslar's own modes. The ten after them come from Aseprite and are drawn
/// with Aseprite's formulas only in a document whose [`Blending`] is [`Blending::Aseprite`];
/// under [`Blending::Pikslar`] each is drawn as its [`BlendMode::stand_in`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Addition,
    Subtract,
    Difference,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    Divide,
}

impl BlendMode {
    /// Every mode, in the order files number them.
    pub const ALL: [BlendMode; 19] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Addition,
        BlendMode::Subtract,
        BlendMode::Difference,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
        BlendMode::Divide,
    ];

    /// The modes Pikslar's own blending has, which every document offers.
    pub const PIKSLAR: [BlendMode; 9] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Addition,
        BlendMode::Subtract,
        BlendMode::Difference,
    ];

    /// The number a file stores for this mode.
    pub fn number(self) -> u8 {
        BlendMode::ALL.iter().position(|&m| m == self).unwrap_or(0) as u8
    }

    /// The mode a file's number stands for, if this version knows it.
    pub fn from_number(n: u8) -> Option<BlendMode> {
        BlendMode::ALL.get(usize::from(n)).copied()
    }

    /// True for a mode only Aseprite-compatible blending draws as itself.
    pub fn aseprite_only(self) -> bool {
        self.stand_in() != self
    }

    /// The mode Pikslar's own blending draws this one as: itself for Pikslar's nine, and the
    /// closest of them for the others, as the `.ase` importer showed them before it had them.
    pub fn stand_in(self) -> BlendMode {
        match self {
            BlendMode::ColorDodge => BlendMode::Screen,
            BlendMode::ColorBurn => BlendMode::Multiply,
            BlendMode::HardLight | BlendMode::SoftLight => BlendMode::Overlay,
            BlendMode::Exclusion => BlendMode::Difference,
            BlendMode::Hue
            | BlendMode::Saturation
            | BlendMode::Color
            | BlendMode::Luminosity
            | BlendMode::Divide => BlendMode::Normal,
            other => other,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::Addition => "Addition",
            BlendMode::Subtract => "Subtract",
            BlendMode::Difference => "Difference",
            BlendMode::ColorDodge => "Color dodge",
            BlendMode::ColorBurn => "Color burn",
            BlendMode::HardLight => "Hard light",
            BlendMode::SoftLight => "Soft light",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
            BlendMode::Divide => "Divide",
        }
    }
}

/// How a document combines its layers. A document setting, stored in the project file.
///
/// `Pikslar` is what every project made in Pikslar uses: a premultiplied picture, built by the
/// arithmetic in [`crate::blend`]. `Aseprite` is for imported Aseprite art that needs it: the
/// same compositor keeps each tile in straight alpha and lays every layer on it with
/// Aseprite's formulas ([`crate::blend_straight`]), so imported art looks as Aseprite shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Blending {
    #[default]
    Pikslar,
    Aseprite,
}

impl Blending {
    /// The name the document's properties show.
    pub fn name(self) -> &'static str {
        match self {
            Blending::Pikslar => "Pikslar",
            Blending::Aseprite => "Aseprite-compatible",
        }
    }

    /// The modes a layer of a document with this blending can be given.
    pub fn modes(self) -> &'static [BlendMode] {
        match self {
            Blending::Pikslar => &BlendMode::PIKSLAR,
            Blending::Aseprite => &BlendMode::ALL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_numbers_are_the_order_and_pikslars_nine_keep_theirs() {
        for (n, mode) in BlendMode::ALL.iter().enumerate() {
            assert_eq!(mode.number() as usize, n);
            assert_eq!(BlendMode::from_number(n as u8), Some(*mode));
        }
        assert_eq!(BlendMode::from_number(19), None);
        assert_eq!(BlendMode::ALL[..9], BlendMode::PIKSLAR);
        assert_eq!(BlendMode::Difference.number(), 8);
        assert_eq!(BlendMode::Divide.number(), 18);
    }

    #[test]
    fn only_the_new_modes_are_aseprite_only_and_each_stands_in_as_one_of_the_nine() {
        for mode in BlendMode::ALL {
            assert_eq!(mode.aseprite_only(), mode.number() >= 9, "{mode:?}");
            assert!(BlendMode::PIKSLAR.contains(&mode.stand_in()));
            assert!(!mode.stand_in().aseprite_only());
        }
        assert_eq!(BlendMode::ColorBurn.stand_in(), BlendMode::Multiply);
        assert_eq!(BlendMode::Hue.stand_in(), BlendMode::Normal);
    }

    #[test]
    fn a_document_offers_the_modes_its_blending_draws() {
        assert_eq!(Blending::default(), Blending::Pikslar);
        assert_eq!(Blending::Pikslar.modes().len(), 9);
        assert_eq!(Blending::Aseprite.modes().len(), 19);
        assert_eq!(Blending::Aseprite.name(), "Aseprite-compatible");
    }
}

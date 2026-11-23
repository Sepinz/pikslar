//! How the onion skin looks: which neighbours show, how strongly, and in which colours.

/// Opacity of a neighbouring frame at full strength, by how many frames away it is.
const STEP_OPACITY: [u32; 3] = [104, 64, 36];

/// The strengths offered, as percentages of [`STEP_OPACITY`].
pub const STRENGTHS: [u32; 5] = [50, 75, 100, 150, 200];
const NORMAL_STRENGTH: usize = 2;

/// The colours earlier and later frames are pulled towards, or none to keep their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnionColors {
    RedBlue,
    GreenPurple,
    Own,
}

impl OnionColors {
    pub const ALL: [OnionColors; 3] = [Self::RedBlue, Self::GreenPurple, Self::Own];

    /// The tints for earlier and later frames, as 0x00RRGGBB.
    pub fn tints(self) -> Option<(u32, u32)> {
        match self {
            Self::RedBlue => Some((0x00e8_5a48, 0x0048_90e8)),
            Self::GreenPurple => Some((0x0040_b858, 0x00a0_60e0)),
            Self::Own => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::RedBlue => "red and blue",
            Self::GreenPurple => "green and purple",
            Self::Own => "their own colours",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::RedBlue => Self::GreenPurple,
            Self::GreenPurple => Self::Own,
            Self::Own => Self::RedBlue,
        }
    }
}

/// What the onion skin shows. One style serves the compositor and the drag preview alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnionStyle {
    /// Earlier frames show.
    pub past: bool,
    /// Later frames show.
    pub future: bool,
    /// An index into [`STRENGTHS`].
    pub strength: usize,
    pub colors: OnionColors,
}

impl Default for OnionStyle {
    fn default() -> Self {
        OnionStyle {
            past: true,
            future: true,
            strength: NORMAL_STRENGTH,
            colors: OnionColors::RedBlue,
        }
    }
}

impl OnionStyle {
    /// The tint, if any, and opacity of a neighbour `distance` frames away; negative is earlier.
    /// Each step further is fainter, and the furthest steps keep the faintest opacity.
    pub fn ghost(&self, distance: i32) -> (Option<u32>, u32) {
        let tint = self
            .colors
            .tints()
            .map(|(past, future)| if distance < 0 { past } else { future });
        let away = (distance.unsigned_abs() as usize).clamp(1, STEP_OPACITY.len());
        let percent = STRENGTHS[self.strength.min(STRENGTHS.len() - 1)];
        let opacity = (STEP_OPACITY[away - 1] * percent / 100).min(255);
        (tint, opacity)
    }

    /// How many frames to show before and after, out of `depth` each way.
    pub fn reach(&self, depth: usize) -> (usize, usize) {
        (
            if self.past { depth } else { 0 },
            if self.future { depth } else { 0 },
        )
    }

    pub fn stronger(&self) -> Option<OnionStyle> {
        (self.strength + 1 < STRENGTHS.len()).then_some(OnionStyle {
            strength: self.strength + 1,
            ..*self
        })
    }

    pub fn fainter(&self) -> Option<OnionStyle> {
        self.strength
            .checked_sub(1)
            .map(|strength| OnionStyle { strength, ..*self })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_normal_style_is_the_one_pikslar_always_had() {
        let s = OnionStyle::default();
        assert_eq!(s.ghost(-1), (Some(0x00e8_5a48), 104));
        assert_eq!(s.ghost(2), (Some(0x0048_90e8), 64));
        assert_eq!(
            s.ghost(-9),
            (Some(0x00e8_5a48), 36),
            "far frames keep the faintest"
        );
        assert_eq!(s.reach(2), (2, 2));
    }

    #[test]
    fn strength_scales_every_step_and_stops_at_its_ends() {
        let s = OnionStyle::default();
        let strong = s
            .stronger()
            .and_then(|s| s.stronger())
            .expect("two steps up");
        assert_eq!(strong.ghost(1).1, 208);
        assert_eq!(strong.ghost(3).1, 72);
        assert_eq!(strong.stronger(), None);
        let faint = s
            .fainter()
            .and_then(|s| s.fainter())
            .expect("two steps down");
        assert_eq!(faint.ghost(-1).1, 52);
        assert_eq!(faint.fainter(), None);
    }

    #[test]
    fn sides_and_colours_can_be_chosen() {
        let s = OnionStyle {
            past: false,
            colors: OnionColors::Own,
            ..OnionStyle::default()
        };
        assert_eq!(s.reach(3), (0, 3));
        assert_eq!(s.ghost(1).0, None);
        let mut c = OnionColors::RedBlue;
        for _ in 0..3 {
            c = c.next();
        }
        assert_eq!(c, OnionColors::RedBlue, "the colours go round");
    }
}

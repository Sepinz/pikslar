//! Marks: 8 by 8, for list rows, tabs and the cel grid.

use super::pack::{sheet, words, Art};

/// The marks, in the order of [`MARK_ART`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkIcon {
    Eye,
    EyeOff,
    Lock,
    Unlock,
    Link,
    FoldRight,
    FoldDown,
    ChevronDown,
    Check,
    RadioDot,
    CelDot,
    CelEmpty,
    KeyFrame,
    Grip,
    Close,
    Modified,
    RadioEmpty,
}

impl MarkIcon {
    pub const ALL: [MarkIcon; MARKS] = {
        use MarkIcon::*;
        [
            Eye,
            EyeOff,
            Lock,
            Unlock,
            Link,
            FoldRight,
            FoldDown,
            ChevronDown,
            Check,
            RadioDot,
            CelDot,
            CelEmpty,
            KeyFrame,
            Grip,
            Close,
            Modified,
            RadioEmpty,
        ]
    };

    pub fn art(self) -> Art {
        Art::new(SIZE as i32, &MARK_ART[self as usize])
    }
}

const SIZE: usize = 8;
const MARKS: usize = 17;

#[rustfmt::skip]
static MARK_ART: [[u32; words(SIZE)]; MARKS] = sheet(SIZE, 8, &[
    // eye   eye off  lock     unlock   link     fold r   fold d   chevron
    "........ ........ ........ ........ ........ ..#..... ........ ........",
    "........ ........ ...##... .....##. ........ ..##.... ........ ........",
    "..####.. #......# ..#..#.. ....#..# .##..##. ..###... ######## ##....##",
    ".#.++.#. .##..##. ..#..#.. ....#... #..##..# ..####.. .######. .##..##.",
    "#.+##+.# ..####.. .######. .######. #..##..# ..####.. ..####.. ..####..",
    ".#.++.#. .#.##.#. .##..##. .#++++#. .##..##. ..###... ...##... ...##...",
    "..####.. #..##..# .######. .#++++#. ........ ..##.... ........ ........",
    "........ ........ .######. .######. ........ ..#..... ........ ........",
    // check radio    cel dot  cel ring key      grip     close    modified
    "........ ..####.. ........ ........ ........ ........ ........ ........",
    ".......# .#....#. ..####.. ..####.. ...##... ........ .##..##. ........",
    "......## #..##..# .######. .#....#. ..####.. ..#..#.. ..####.. ...##...",
    "#....##. #.####.# .######. .#....#. .######. ........ ...##... ..####..",
    "##..##.. #.####.# .######. .#....#. .######. ..#..#.. ...##... ..####..",
    ".####... #..##..# .######. .#....#. ..####.. ........ ..####.. ...##...",
    "..##.... .#....#. ..####.. ..####.. ...##... ..#..#.. .##..##. ........",
    "........ ..####.. ........ ........ ........ ........ ........ ........",
    // radio, not chosen
    "..####..",
    ".#....#.",
    "#......#",
    "#......#",
    "#......#",
    "#......#",
    ".#....#.",
    "..####..",
]);

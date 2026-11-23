# Pikslar keyboard shortcuts

Every shortcut can be changed in View, Keyboard shortcuts. The menus show the current one beside each command.

## Drawing, frames and layers

| Key | Action |
|---|---|
| B, E, G, I, L, U | Pencil, eraser, fill, picker, line, rectangle |
| M, Q, W, V | Select box, lasso, magic wand, move |
| Right-drag, Alt+click | Erase, pick a color |
| Shift while drawing | Line Rhythm for this stroke |
| `[` and `]` | Brush size |
| Wheel, `+`, `-`, `0` | Zoom, fit to window |
| Space-drag, middle-drag | Pan |
| Ctrl+Z, Ctrl+Y or Ctrl+Shift+Z | Undo, redo |
| Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S | New, open, save, save as |
| Ctrl+E, Ctrl+Shift+E | Export frame, export sprite sheet |
| `,` and `.`, Left and Right | Previous and next frame |
| Home, End | First and last frame |
| Up, Down | Layer above, layer below |
| Enter, O | Play, onion skin |
| Shift+O | Onion skin depth |
| N, D, Shift+Delete or Shift+Backspace | New, duplicate and remove frames |
| Shift+Left and Right, `<` and `>` | Move the frame |
| Shift+Up, Shift+Down | Move the layer |
| Space, Esc | Hold to pan, stop playing or cancel |
| K, Shift+K | Hold a drawing across frames |
| T, Shift+T | Tag frames, change a tag's direction |

With a selection:

| Key | Action |
|---|---|
| Shift or Alt while selecting | Add to, or subtract from, the selection |
| Ctrl with the wand | Select that color everywhere, not only the connected area |
| Drag inside, Ctrl-drag inside | Move the pixels, move a copy |
| Arrows, Shift+arrows | Nudge picked-up pixels by 1, by 10. Use the Move tool to pick them up |
| Shift+H, Shift+V, R, Shift+R | Flip, rotate clockwise and counterclockwise |
| Enter, Esc | Apply the move or transform, cancel it |
| Ctrl+C, Ctrl+X, Ctrl+V | Copy, cut, paste |
| Ctrl+A, Ctrl+D, Ctrl+Shift+I | Select all, deselect, invert |
| Delete, Backspace | Clear the selected pixels |

## Command line

| Flag | Purpose |
|---|---|
| `--size N` | Start with an N by N canvas |
| `--zoom N`, `--window X Y W H` | Starting zoom, window position and size |
| `--scale N` | Force the interface scale instead of reading the display |
| `--dpi N` | Pretend the display reports this DPI, to check layouts on one machine |
| `--no-recovery`, `--recovery-dir PATH` | Turn crash recovery off, or keep its files elsewhere |


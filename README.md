# Romo

A native, offline whiteboard written in Rust with **GPUI**. An infinite black canvas for notes, shapes, drawings, and connected ideas. The visual rules are in [design.md](design.md).

## Run with Nix

For the quickest build and launch:

```sh
nix build .#fast
./result/bin/romo
```

The fast package uses the same fully optimized release profile as the default
package and skips tests. Both packages share the compiled dependency cache, so
switching between them reuses GPUI and the other compiled crates. The first build
still compiles optimized dependencies. To build and run with tests, use:

```sh
nix run
```

From another directory:

```sh
nix run github:Kalkaro/romo
```

Open a saved board:

```sh
nix run . -- /path/to/ideas.romo
```

Nix builds the Rust application and provides its native libraries. Compiled Rust
dependencies are cached separately from the application, so changing Romo's source
reuses GPUI and the other compiled crates. The first build still compiles those
dependencies; later launches use the built package. Documentation changes do not
invalidate the package. Flakes and `nix-command` must be enabled. Linux requires a
graphical session and a working Vulkan driver. The flake also defines macOS
packages; the Linux build is the primary tested target.

## The board

- Pan and zoom around an infinite canvas.
- Create sticky notes, text, rectangles, ellipses, diamonds, and frames.
- Draw smooth curves with rounded pen tips; new drawings stay unselected. Drag connectors between objects. Attached connectors follow moved objects.
- Erase pen strokes with the eraser (**E**): drag across strokes to remove them whole. A short fading trail follows the eraser. Shapes, notes, text, images, and connectors are never erased. Each drag is one undo step.
- Draw with a graphics tablet. On Wayland, Romo reads the pen through the tablet protocol, so it works in compositors such as Hyprland that don't turn pens into a mouse, including with OpenTabletDriver's artist mode. The pen acts like a mouse: the tip clicks and draws, the lower barrel button pans, and the upper one opens the object toolbar. Pressure is not used.
- Scale drawings: select one or more pen strokes and drag the corner handle. Scaling is uniform; hold Shift to stretch freely. Strokes keep their drawn width when a drawing grows and thin out when it shrinks below its drawn size.
- Move and resize objects. Drag a frame to move its contained objects.
- Select several objects with Shift-click or a selection box; they share one outline around their combined bounds. Drag anywhere inside existing selection bounds to move them, including empty space around a drawing.
- Click with the text tool for a text box that grows with what you type, or drag out a box to wrap text at its width. A text box left empty disappears.
- Add text or double-click an object to edit directly on the canvas. Text updates and saves as you type; click elsewhere, press Escape, or press Ctrl+Enter to finish. The native editor supports Unicode, text selection, clipboard operations, and IME composition. Each editing session is one undo step.
- Right-click an object or selection for a horizontal icon toolbar with color, text, dimensions, line width, layering, duplicate, and delete controls. Shift+F10 also opens it.
- Paste images from the system clipboard with Ctrl+V, or use the image tool. Copy images to other apps. Images stay embedded in saved boards and SVG exports. Corner resizing preserves aspect ratio; Shift allows free resizing.
- Undo, redo, duplicate, copy, paste, and delete. Pasted objects and images land centered under the pointer; pasting again without moving it cascades the copies. With the pointer off the board, copies go beside the originals.
- Press **Y** to toggle declutter mode: the controls fade out or back in over 200 ms, with the board position preserved. Press Y again during a transition to reverse the fade. Selection outlines, resizing, drawing, snapping, keyboard tools, and the right-click object toolbar still work, and notices such as "Board saved" still appear. While editing text, Y types the letter.
- Large boards stay smooth when zoomed out: text too small to read is drawn as soft lines, text is shaped once and scaled, pen strokes are tessellated once and reused, and strokes of one color drawn in a row are painted together. Copies of a board, such as undo steps, share their pen strokes instead of duplicating them.
- Align objects and groups to other objects with snapping guides. Pen strokes that touch or overlap count as one drawing, and objects align to the drawing as a whole; lone strokes and connectors are not snap targets. Snapping is on by default; turn it off in Settings or hold Alt during a drag to bypass it. Toggle the grid and navigate with the minimap.
- Save and open `.romo` files; export the full board to SVG.

The top bar floats in the top-left corner: **Save**, the **File** menu, the board name, then undo and redo. Click the board name to choose a saved board, create a new one, import a board, or rename it. **Save** writes directly to the library; **File → Export board** writes a portable `.romo` copy.

Romo stores boards in `~/.local/share/romo/boards/` on Linux, with `session.json` and `settings.json` alongside that folder. Boards are compressed, so a drawing-heavy board takes about a tenth of the space it once did; `session.json` only records which board was open. Changes are saved about once a second in the background, and on exit, using atomic file replacement. Switching boards first saves your current work. Restarting restores the last board and viewport. Existing sessions and imported files are copied into the library; original files and unreadable recovery files are preserved. Set `ROMO_DATA_DIR` to choose another folder. Native file dialogs use your desktop portal, with the packaged Zenity fallback on Linux.

| Action | Shortcut |
| --- | --- |
| Select / Hand | V / H |
| Sticky note / Text | N / T |
| Rectangle / Ellipse / Diamond | R / O / D |
| Connector / Pen / Eraser / Frame | L / P or B / E / F |
| Pan | Space + drag, or middle-button drag |
| Zoom | Wheel, + / −, or Ctrl + trackpad scroll |
| Trackpad pan | Two-finger scroll |
| Reset zoom / Fit board | 1 / 2 |
| Edit selected object | Enter |
| Move selected objects | Arrow keys; Shift for 1 px steps |
| Select all / Deselect | Ctrl+A / Escape |
| Copy / Paste / Duplicate | Ctrl+C / Ctrl+V / Ctrl+D |
| Undo / Redo | Ctrl+Z / Ctrl+Shift+Z |
| New / Open / Save | Ctrl+N / Ctrl+O / Ctrl+S |
| Export board / Export SVG | Ctrl+Shift+S / Ctrl+E |
| Grid / Minimap | G / M |
| Declutter / Show controls | Y |
| Finish live text editing | Click elsewhere, Escape, or Ctrl+Enter |
| Delete | X, Delete, or Backspace |

Use ⌘ instead of Ctrl on macOS. The Settings button includes the persistent object-snapping toggle.

## Develop

```sh
nix develop
cargo run
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

For incremental local builds without entering the shell interactively:

```sh
nix develop -c cargo build --locked
nix develop -c cargo run --locked -- /path/to/ideas.romo
```

Cargo keeps compiled crates and incremental build data in `target/`. Keep that
directory between edits. Development builds omit debug symbols to reduce compile
and link work. Use `cargo run --release` for the same runtime optimization as the
Nix packages.

Build the standalone Nix package, including its tests:

```sh
nix build
./result/bin/romo
```

Run a native launch check in a graphical session without touching the saved session:

```sh
nix run . -- --smoke-test
```

Measure rendering on a generated board of about 900 objects. The window animates
zooming and panning, then prints frames per second and board paint times:

```sh
cargo run --release -- --bench
```

To measure one of your own boards instead, pass its file. The benchmark only
reads it:

```sh
cargo run --release -- --bench ~/.local/share/romo/boards/board-….romo
```

The app has no accounts, networking, or collaboration features. Board files are gzip-compressed, versioned JSON (`zcat board.romo` shows it), with each pen stroke's points as one flat list of coordinates. Romo still opens version 1 boards, plain JSON with an object per point, and saves them in the new format. SVG export preserves objects and editable text, with approximate font wrapping.

## License

MIT. The bundled Inter font is licensed under the SIL Open Font License; see [assets/Inter-LICENSE.txt](assets/Inter-LICENSE.txt). GPUI and other dependencies retain their respective licenses.

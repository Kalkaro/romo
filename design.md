# Romo — Black board design

Romo is a native GPUI whiteboard for local work. Keep the canvas central and make controls appear only when they serve the current action.

## Color and type

Use `#09090B` for the canvas, `#18181B` for floating surfaces, `#27272A` for borders and hover, `#3F3F46` for object outlines, `#A1A1AA` for secondary text, and `#FAFAFA` for text, icons, and selection outlines. Active tool buttons and open menus may invert to white with a black icon. Red `#DC2626` is reserved for errors.

Board objects use the existing eight grayscale swatches. Imported images retain their original colors and transparency.

Use bundled Inter. Interface text is 13–15 px, board text defaults to 32 px, and note text to 20 px. Board content scales with zoom. Use medium or semibold titles and regular body text.

## Layout

There is no header: the board fills the window and every control floats above it. The top bar sits in the top-left corner and uses the same panel and icon buttons as the tools: **Save** in the corner, the File menu, the Romo mark with the current board name as a simple board picker, then undo and redo. Long board names truncate with an ellipsis. Menus open directly below their buttons. Do not display “Draft saved locally,” save-state labels, or the bottom tool-instruction sentence. The picker lists stored boards and provides New board, Import board, and Rename current board.

Tools float along the left edge, centered between the corner panels; their spacing tightens when the window is too short for the regular rhythm. Help, Settings, grid, and object count sit at the bottom left. Zoom, fit, and the optional minimap sit at the bottom right. Notices appear in the top-right corner. Use 24 px edge spacing, 36 px icon hit areas, 4 px button corners, 8 px panel corners, and 1 px borders.

## Mark and icons

The Romo mark is a lowercase “r” whose arm runs into an “o” ring, like a connector reaching an idea. The app icon (`assets/romo.svg`) sets the white mark on a near-black rounded tile; the monochrome mark (`assets/icons/logo.svg`) appears in the top bar and the shortcut sheet. Interface icons use 1.5 px strokes in a 20 px box. The hand is a bundled SVG (`assets/icons/hand.svg`) with rounded fingers.

Do not reserve a sidebar for object properties. **Right-click an object or an existing selection** to open a compact horizontal toolbar near its bounds; Shift+F10 also opens it. Use icons with hover labels for color, text editing, dimensions, layer order, duplicate, and delete. Color and size controls open small popovers. Normal selection does not automatically open the toolbar. Close it when manipulating the board, editing text, or dismissing the selection.

Every floating control, menu, popup, and minimap must block pointer events from reaching the board beneath it. Pressing a control must never begin a board selection, drawing, or drag gesture. Menus must remain usable on smaller windows.

## Selection and drawing

Use thin white selection bounds, square resize handles, and a low-opacity marquee. For multiple objects, show their combined bounds. Dragging **anywhere inside an existing selection rectangle** moves the selection, including empty space around freehand strokes. Moving a group preserves relative positions.

New pen strokes, connectors, and shapes are not selected automatically. Pen curves use smooth quadratic segments with round caps and joins; taps create dots. Explicitly selecting a drawing displays its selection rectangle normally. While a note or text box is being dragged out, it shows no placeholder.

A selection made only of pen strokes shows handles on its combined bounds. Dragging the bottom-right handle scales the drawing uniformly from its top-left corner; Shift stretches it freely. Strokes keep the width they were drawn with when a drawing grows beyond its drawn size and thin out proportionally when it shrinks below it. The line-width control changes the visible width.

The eraser (**E**) removes whole pen strokes that it touches, including strokes crossed between pointer samples. It never erases shapes, notes, text, images, or connectors. A thin circular outline marks its reach at the pointer, and a soft translucent trail narrows behind it and fades within a quarter second. Each drag is one undo step.

Clicking with the text tool creates text whose box follows its longest line, so its selection hugs the words. Dragging out a text box, or resizing one, fixes its width and wraps the text. A text box left empty when editing ends disappears; if it was just created, it leaves no undo step. Text on the canvas matches the inline editor exactly once the zoom settles.

Graphics tablet pens act as a mouse. On Wayland, read them through the tablet protocol on GPUI's own connection, since GPUI doesn't and compositors such as Hyprland don't emulate a pointer for pens. The tip is the left button, the lower barrel button the middle one, and the upper barrel button the right one. Controls record where they are painted so pen taps reach them. Pressure and tilt are ignored. **X**, Delete, and Backspace delete the selection.

Text is edited **directly on the canvas**, without a text-entry popup. Preserve Unicode, clipboard operations, multiline input, and IME composition. Click elsewhere, Escape, or Ctrl+Enter finishes editing; group the editing session into one undo step. Renaming a board may use a simple dialog.

## Board-only mode

**Y** toggles board-only mode without moving the board. Fade the interface out and back in over 200 ms with gentle easing; pressing Y during the transition reverses the fade from its current opacity. Fade application chrome only, keeping board objects, selection outlines, handles, and guides fully visible. The board always fills the window, so the fade never shifts it. Hide the top bar, toolbars, menus, settings, hints, minimap, and other application chrome. The object toolbar still opens with right-click or Shift+F10, at full opacity. Preserve all board functionality: selection outlines, marquee selection, movement, resize handles, snapping guides, drawing, keyboard shortcuts, and inline text editing. While typing, Y inserts a letter.

## Images and snapping

Paste clipboard images with Ctrl+V or add an image using the image tool. Images are board objects: select, move, resize, duplicate, copy, paste, delete, and align them like other objects. Preserve aspect ratio during corner resizing; Shift permits free resizing. Copying a single image places native image data on the system clipboard for other applications. Copying several objects preserves their structure within Romo. Pasted objects land centered under the pointer while it is over the board; pasting again in the same spot cascades the copies so none hides another. Embed images in portable board and SVG exports.

Object snapping is **on by default**. Moving selections align to nearby object edges and centers, including images. Pen strokes that touch or overlap form one drawing, and objects align to that drawing's bounds as a whole; lone strokes and connectors are not snap targets. A selection that contains drawings still snaps by its combined bounds. Shape creation and resizing also align to these targets. Show subtle alignment guides during manipulation and remove them on release. Measure the snap threshold in screen pixels so zoom does not change its feel. Snap groups as a unit. Settings provides a persistent opt-out; holding Alt temporarily bypasses snapping.

## Rendering

Panning and zooming must stay at the display's frame rate on large boards. Text smaller than 5 px on screen is drawn as soft bars. Larger text is shaped once at a reference size and painted at any zoom by scaling glyph positions. While the zoom changes, glyph images come in 24 sizes per doubling; 150 ms after it stops, they render at exact sizes. Pen strokes are tessellated once per zoom octave and reused; refinement after a zoom runs within a few milliseconds per frame. Outlines, frames, guides, and circles are single quads, and the grid spaces its dots at least 24 px apart. `romo --bench` measures frame rate and paint time on a generated board.

## Local storage

Keep boards in the operating system's normal Romo app data directory. On Linux this is `~/.local/share/romo/boards/`; settings and session recovery live alongside it. `ROMO_DATA_DIR` may override the directory. Board filenames use stable identities independent of their titles.

Save writes the current board directly to the library. Autosave runs about once a second on a background thread, so saving a large board never stalls the board, and on exit. Save the current board before switching; if saving fails, keep that board open and display the error. Restore the last edited board and its viewport on restart. Import existing sessions and external files into the library without overwriting the original files. Export board writes a portable `.romo` copy; SVG export includes embedded images. Use atomic replacement and preserve unreadable recovery files.

Accounts, online sync, and collaboration are outside this version's scope.

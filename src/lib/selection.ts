// xterm's selection is cell-based: every selected row is returned padded out to
// the rightmost selected column, so copying TUI/box-drawing output drags along a
// run of trailing spaces on each line. Strip that padding (spaces/tabs only) so
// pasted text matches what the user visually selected. A trailing \r (Windows
// line ends) is preserved.
export function trimSelectionTrailing(selection: string): string {
  return selection
    .split('\n')
    .map((line) => line.replace(/[ \t]+(\r?)$/, '$1'))
    .join('\n')
}

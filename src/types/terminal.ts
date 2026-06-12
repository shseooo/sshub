export interface TerminalPane {
  /** PTY session id — also the event channel suffix */
  sessionId: string;
  serverId: number | null;
  label: string;
}

export interface TerminalTab {
  id: string;
  panes: TerminalPane[];
  /** Split axis: 'row' = side by side, 'column' = stacked */
  direction: 'row' | 'column';
  /** Per-pane size as a percentage of the tab, summing to ~100 */
  sizes: number[];
}

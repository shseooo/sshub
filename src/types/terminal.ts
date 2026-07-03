/** A single terminal session pane (tree leaf). */
export interface TerminalLeaf {
  type: 'leaf'
  /** PTY session id — also the event channel suffix */
  sessionId: string
  serverId: number | null
  label: string
  /** Transient (not persisted): when this leaf was just created by splitting a
   *  local pane, the source pane's session id. The new local shell starts in that
   *  pane's current working directory. Consumed once at first session start. */
  cwdFromSessionId?: string
}

/** A split container holding child panes side by side ('row') or stacked ('column'). */
export interface TerminalSplit {
  type: 'split'
  id: string
  direction: 'row' | 'column'
  children: PaneNode[]
  /** Per-child size as a percentage of this split, summing to ~100 */
  sizes: number[]
}

export type PaneNode = TerminalLeaf | TerminalSplit

export interface TerminalTab {
  id: string
  /** Root of the pane tree — a lone leaf, or nested splits. */
  root: PaneNode
  /** Optional custom tab name; falls back to the first leaf's label. */
  name?: string
}

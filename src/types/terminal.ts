export interface TerminalSession {
  id: string;
  serverId: number;
  serverName: string;
  serverHost: string;
  serverUser: string;
  isActive: boolean;
  isConnecting: boolean;
  error: string | null;
}

export interface TerminalTab {
  id: string;
  sessionId: string | null;
  serverId: number | null;
  serverName: string;
}
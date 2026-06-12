export type AuthType = 'key' | 'password' | 'pem';

export interface Server {
  id: number;
  name: string;
  host: string;
  port: number;
  username: string;
  authType: AuthType;
  keyId: number | null;
  pemData: string | null;
  groupName: string | null;
  /** JSON-encoded string array */
  tags: string | null;
  isFavorite: boolean;
  notes: string | null;
  lastConnectedAt: string | null;
  createdAt: string | null;
  updatedAt: string | null;
}

export interface CreateServerDto {
  name: string;
  host: string;
  port?: number;
  username: string;
  authType: AuthType;
  keyId?: number;
  pemData?: string;
  groupName?: string;
  /** JSON-encoded string array */
  tags?: string;
  notes?: string;
}

export interface UpdateServerDto extends Partial<CreateServerDto> {
  id: number;
}

export interface SSHConfigEntry {
  id: number;
  hostPattern: string;
  hostname: string | null;
  user: string | null;
  port: number | null;
  identityFile: string | null;
  forwardAgent: string | null;
  localForward: string[];
  remoteForward: string[];
  otherOptions: Record<string, string>;
  synced: boolean;
}

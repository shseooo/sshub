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
  passwordHash: string | null;
  passwordSaved: boolean;
  groupName: string;
  tags: string[];
  isFavorite: boolean;
  notes: string;
  createdAt: string;
  updatedAt: string;
}

export interface CreateServerDto {
  name: string;
  host: string;
  port?: number;
  username: string;
  authType: AuthType;
  keyId?: number;
  pemData?: string;
  password?: string;
  groupName?: string;
  tags?: string[];
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
export type KeyType = 'ed25519' | 'rsa' | 'ecdsa' | 'dsa';

export interface SshKey {
  id: number;
  name: string;
  publicKey: string;
  pemData: string | null;
  keyType: KeyType;
  keySize: number;
  passphraseProtected: boolean;
  createdAt: string | null;
  /** Whether the private key file exists on this machine (set by get_ssh_keys). */
  hasPrivateFile?: boolean;
}

export interface CreateKeyDto {
  name: string;
  keyType: KeyType;
  keySize?: number;
  passphrase?: string;
}

export interface ImportKeyDto {
  name: string;
  publicKey: string;
  privateKey?: string;
  pemData?: string;
  keyType: KeyType;
  passphrase?: string;
}

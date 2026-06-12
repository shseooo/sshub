export type KeyType = 'ed25519' | 'rsa' | 'ecdsa' | 'dsa';

export interface SshKey {
  id: number;
  name: string;
  publicKey: string;
  pemData: string | null;
  keyType: KeyType;
  keySize: number;
  passphraseProtected: boolean;
  createdAt: string;
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
  privateKey: string;
  pemData?: string;
  keyType: KeyType;
  passphrase?: string;
}
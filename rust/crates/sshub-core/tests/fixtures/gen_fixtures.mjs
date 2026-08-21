// Node(Electron) 구현이 실제로 만들어내는 바이트를 픽스처로 고정한다.
// Rust 포팅이 같은 바이트를 읽고 쓰는지 compat_*.rs 가 이 파일들로 검증한다.
//
// 실행(레포 루트에서): bun rust/crates/sshub-core/tests/fixtures/gen_fixtures.mjs
// 산출물은 커밋한다 — 재생성은 Electron 구현이 바뀔 때만.
import { randomBytes, scryptSync, createCipheriv } from 'node:crypto'
import { writeFileSync } from 'node:fs'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const PASSPHRASE = 'test-pass'

// ---- electron/lib/crypto.ts 와 동일 로직 (그 파일에서 옮겨온 것) ----
const MAGIC = 'sshub-enc-v1'
function encryptBundle(plaintext, passphrase) {
  const salt = randomBytes(16)
  const iv = randomBytes(12)
  const key = scryptSync(passphrase, salt, 32)
  const cipher = createCipheriv('aes-256-gcm', key, iv)
  const ct = Buffer.concat([cipher.update(plaintext, 'utf8'), cipher.final()])
  const tag = cipher.getAuthTag()
  return JSON.stringify({
    magic: MAGIC,
    salt: salt.toString('base64'),
    iv: iv.toString('base64'),
    ct: ct.toString('base64'),
    tag: tag.toString('base64'),
  })
}

// ---- 모든 필드 변형(널/즐겨찾기/태그/그룹/proxyJump/pem)을 담은 스토어 ----
const store = {
  nextServerId: 4,
  nextKeyId: 3,
  servers: [
    {
      id: 1,
      name: 'prod-web',
      host: '10.0.0.1',
      port: 22,
      username: 'deploy',
      authType: 'key',
      keyId: 1,
      pemData: null,
      proxyJump: 'bastion.example.com',
      groupName: 'production',
      tags: '["web","nginx"]',
      isFavorite: true,
      notes: '메모 — 한글 & "quotes" \\ backslash',
      lastConnectedAt: '2026-08-20T10:11:12.345Z',
      createdAt: '2026-01-02T03:04:05.678Z',
      updatedAt: '2026-08-20T10:11:12.345Z',
    },
    {
      id: 2,
      name: 'db-replica',
      host: 'db.internal',
      port: 2222,
      username: 'postgres',
      authType: 'password',
      keyId: null,
      pemData: null,
      proxyJump: null,
      groupName: null,
      tags: null,
      isFavorite: false,
      notes: null,
      lastConnectedAt: null,
      createdAt: '2026-02-03T04:05:06.007Z',
      updatedAt: '2026-02-03T04:05:06.007Z',
    },
    {
      id: 3,
      name: 'legacy-pem',
      host: '203.0.113.9',
      port: 22,
      username: 'ec2-user',
      authType: 'pem',
      keyId: null,
      pemData: null,
      proxyJump: null,
      groupName: 'aws',
      tags: '[]',
      isFavorite: false,
      notes: null,
      lastConnectedAt: null,
      createdAt: '2026-03-04T05:06:07.089Z',
      updatedAt: '2026-03-04T05:06:07.089Z',
    },
  ],
  keys: [
    {
      id: 1,
      name: 'work-ed25519',
      publicKey: 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExampleKeyDataHere work@example',
      pemData: null,
      keyType: 'ed25519',
      keySize: 256,
      passphraseProtected: true,
      createdAt: '2026-01-01T00:00:00.000Z',
    },
    {
      id: 2,
      name: 'legacy rsa key',
      publicKey: 'ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQExampleRsa legacy@example',
      pemData: null,
      keyType: 'rsa',
      keySize: 3072,
      passphraseProtected: false,
      createdAt: '2026-01-05T06:07:08.900Z',
    },
  ],
}

// electron/store.ts 의 save(): JSON.stringify(data, null, 2)
writeFileSync(`${here}/node_sshub.json`, JSON.stringify(store, null, 2))

// electron/lib/bundleOps.ts buildExportBundle + backup.ts 평문 export
const bundle = {
  version: 1,
  servers: store.servers,
  keys: store.keys,
  shortcuts: { newTab: 'meta+KeyT', splitRight: 'meta+KeyD' },
}
writeFileSync(`${here}/node_plain_export.json`, JSON.stringify(bundle, null, 2))

// backup.ts 암호화 export: SecureBundle 을 envelope 로
const secure = {
  bundle,
  privateKeys: [{ name: 'work-ed25519', pem: '-----BEGIN OPENSSH PRIVATE KEY-----\nZmFrZQ==\n-----END OPENSSH PRIVATE KEY-----\n' }],
}
const securePlaintext = JSON.stringify(secure)
writeFileSync(`${here}/node_envelope.enc`, encryptBundle(securePlaintext, PASSPHRASE))
// Rust 가 복호화 결과를 비교할 원문 (envelope 안에 들어간 그 문자열 그대로)
writeFileSync(`${here}/node_envelope_plaintext.json`, securePlaintext)

console.log('fixtures written to', here)

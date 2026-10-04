import type { Device, StoredDevice } from '../Domain/device.js'

export interface AccessRepository {
  createToken(digest: string, expiresAt: number, now: number): void
  enroll(tokenDigest: string, deviceId: string, secretDigest: string, name: string, now: number): 'ok' | 'invalid' | 'retired'
  authenticate(deviceId: string, secretDigest: string): Device | null
  listDevices(): StoredDevice[]
  markDeleting(deviceId: string): boolean
  finishRemoval(deviceId: string): void
  listDeleting(): string[]
}

export interface CredentialSecrets {
  generate(): string
  digest(value: string): string
}

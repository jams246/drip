import type { PurgeDeviceMirrors } from '../../synchronization/Application/synchronization.js'
import { AccessError } from '../Domain/access-error.js'
import { DEVICE_ID_PATTERN, REGISTRATION_LIFETIME_MS, SECRET_PATTERN } from '../Domain/device.js'
import type { Device, Enrollment } from '../Domain/device.js'
import type { AccessRepository, CredentialSecrets } from './ports.js'

const BEARER_PREFIX = 'Bearer '
const MAX_NAME_LENGTH = 100
// oxlint-disable-next-line eslint/no-control-regex -- Device names must not contain control characters.
const CONTROL_CHARACTERS = /[\x00-\x1f\x7f]/ // Non-printing ASCII characters, including NUL, tab, newline, escape, and DEL.

export class AccessApplication {
  constructor(
    private readonly repository: AccessRepository,
    private readonly secrets: CredentialSecrets,
    private readonly mirrors: PurgeDeviceMirrors,
    private readonly now: () => number = Date.now
  ) {}

  createToken(): { token: string; expiresAt: number } {
    const token = this.secrets.generate()
    const now = this.now()
    const expiresAt = now + REGISTRATION_LIFETIME_MS
    this.repository.createToken(this.secrets.digest(token), expiresAt, now)
    return { token, expiresAt }
  }

  enroll(input: unknown): Device {
    const enrollment = this.validateEnrollment(input)
    const result = this.repository.enroll(
      this.secrets.digest(enrollment.token),
      enrollment.deviceId,
      this.secrets.digest(enrollment.secret),
      enrollment.name,
      this.now()
    )
    if (result === 'retired') throw new AccessError('device_retired', 'This device was removed. Register a new device identity.')
    if (result !== 'ok') throw new AccessError('invalid_token', 'The registration token is expired, used, or invalid.')
    return { deviceId: enrollment.deviceId, name: enrollment.name }
  }

  authenticate(authorization: string | undefined): Device {
    if (!authorization?.startsWith(BEARER_PREFIX)) throw this.unauthorized()
    const parts = authorization.slice(BEARER_PREFIX.length).split('.')
    if (parts.length !== 2 || !DEVICE_ID_PATTERN.test(parts[0]) || !SECRET_PATTERN.test(parts[1])) throw this.unauthorized()
    const device = this.repository.authenticate(parts[0], this.secrets.digest(parts[1]))
    if (!device) throw this.unauthorized()
    return { deviceId: device.deviceId, name: device.name }
  }

  listDevices() {
    return this.repository.listDevices()
  }

  async removeDevice(deviceId: string): Promise<void> {
    if (!DEVICE_ID_PATTERN.test(deviceId)) throw new AccessError('bad_request', 'A canonical device UUID is required.')
    if (!this.repository.markDeleting(deviceId)) throw new AccessError('device_not_found', 'The device does not exist.')
    await this.mirrors.purgeDeviceMirrors(deviceId)
    this.repository.finishRemoval(deviceId)
  }

  async recoverRemovals(): Promise<void> {
    for (const deviceId of this.repository.listDeleting()) {
      await this.mirrors.purgeDeviceMirrors(deviceId)
      this.repository.finishRemoval(deviceId)
    }
  }

  private validateEnrollment(input: unknown): Enrollment {
    if (!input || typeof input !== 'object') throw new AccessError('bad_request', 'An enrollment object is required.')
    const { token, deviceId, secret, name } = input as Partial<Enrollment>
    if (typeof token !== 'string' || !SECRET_PATTERN.test(token) || typeof secret !== 'string' || !SECRET_PATTERN.test(secret)) {
      throw new AccessError('bad_request', 'Token and secret must each contain 64 lowercase hexadecimal characters.')
    }
    if (typeof deviceId !== 'string' || !DEVICE_ID_PATTERN.test(deviceId)) throw new AccessError('bad_request', 'A canonical device UUID is required.')
    if (typeof name !== 'string' || name.trim().length === 0 || name.length > MAX_NAME_LENGTH || CONTROL_CHARACTERS.test(name)) {
      throw new AccessError('bad_request', 'A device name of 1 to 100 characters is required.')
    }
    return { token, deviceId, secret, name }
  }

  private unauthorized(): AccessError {
    return new AccessError('unauthorized', 'A valid device credential is required.')
  }
}

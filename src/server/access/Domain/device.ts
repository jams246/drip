export interface Device {
  deviceId: string
  name: string
}

export interface Enrollment extends Device {
  token: string
  secret: string
}

export interface StoredDevice extends Device {
  state: 'active' | 'deleting'
  createdAt: number
}

export const REGISTRATION_LIFETIME_MS = 900_000
export const DEVICE_ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
export const SECRET_PATTERN = /^[0-9a-f]{64}$/

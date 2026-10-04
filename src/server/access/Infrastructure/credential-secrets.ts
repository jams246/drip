import { createHash, randomBytes } from 'node:crypto'
import type { CredentialSecrets } from '../Application/ports.js'

const CREDENTIAL_BYTES = 32

export class SecureCredentials implements CredentialSecrets {
  generate(): string {
    return randomBytes(CREDENTIAL_BYTES).toString('hex')
  }

  digest(value: string): string {
    return createHash('sha256').update(value).digest('hex')
  }
}

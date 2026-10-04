import type { FileEntry, Offer, RootRegistration } from '../../../protocol/sync.js'
export type {
  Change,
  DirectoryEntry,
  FileEntry,
  FileMove,
  HeadsPage,
  MirrorEntry,
  Offer,
  OperationReceipt,
  RegionFingerprint,
  RegionPage,
  RootRegistration
} from '../../../protocol/sync.js'

export interface Root extends RootRegistration {
  deviceId: string
  revision: number
  retired: boolean
}

export interface Operation extends Offer {
  rootId: string
  deviceId: string
  status: 'offered' | 'publishing' | 'committed' | 'aborted'
  revision: number
  planComplete: boolean
  stageReady: boolean
  metadataOnly: boolean
  basis?: FileEntry
}

export interface PlannedRegion {
  offset: number
  length: number
  hash: string
  basisOffset: number | null
  uploaded: boolean
}

export const canonicalPath = (path: string) => path.toLowerCase()

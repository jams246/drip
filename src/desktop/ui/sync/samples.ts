import type { ActivityEntry, SyncFile, WatchItem } from './types'

const percentMaximum = 100
const recent = new Date()
const minute = 60_000

export const sampleWatches: WatchItem[] = [
  { id: 'documents', name: 'Documents', path: 'C:\\Users\\Jason\\Documents', kind: 'folder' },
  { id: 'photos', name: 'Photos', path: 'C:\\Users\\Jason\\Pictures\\Photos', kind: 'folder' },
  { id: 'projects', name: 'Projects', path: 'C:\\Users\\Jason\\Projects', kind: 'folder' },
  { id: 'notes', name: 'Notes.md', path: 'C:\\Users\\Jason\\Desktop\\Notes.md', kind: 'file' }
]

export const sampleFiles: SyncFile[] = [
  {
    id: 'video',
    watchId: 'photos',
    name: 'Coastline.mp4',
    path: 'Photos / Ireland / Coastline.mp4',
    kind: 'file',
    status: 'syncing',
    stage: 'Uploading',
    value: 0,
    max: percentMaximum,
    detail: '428 MB'
  },
  {
    id: 'archive',
    watchId: 'projects',
    name: 'Design archive.zip',
    path: 'Projects / Studio / Design archive.zip',
    kind: 'file',
    status: 'preparing',
    stage: 'Scanning chunks',
    value: 28,
    max: percentMaximum,
    detail: '86.2 MB'
  },
  {
    id: 'photo',
    watchId: 'photos',
    name: 'Morning light.jpg',
    path: 'Photos / Ireland / Morning light.jpg',
    kind: 'file',
    status: 'preparing',
    stage: 'Hashing file',
    max: percentMaximum,
    detail: '12.4 MB'
  },
  {
    id: 'budget',
    watchId: 'documents',
    name: 'Budget.xlsx',
    path: 'Documents / Personal / Budget.xlsx',
    kind: 'file',
    status: 'error',
    stage: 'Needs attention',
    value: 0,
    max: percentMaximum,
    detail: 'Could not read this file.',
    errorActivityId: 'budget-error'
  },
  {
    id: 'brief',
    watchId: 'documents',
    name: 'Project brief.pdf',
    path: 'Documents / Studio / Project brief.pdf',
    kind: 'file',
    status: 'synced',
    stage: 'Synced',
    value: percentMaximum,
    max: percentMaximum,
    detail: '2.8 MB'
  },
  {
    id: 'projects-folder',
    watchId: 'projects',
    name: 'Website assets',
    path: 'Projects / Website assets',
    kind: 'folder',
    status: 'synced',
    stage: 'Synced',
    value: percentMaximum,
    max: percentMaximum,
    detail: '18 files'
  },
  {
    id: 'notes-file',
    watchId: 'notes',
    name: 'Notes.md',
    path: 'Desktop / Notes.md',
    kind: 'file',
    status: 'synced',
    stage: 'Synced',
    value: percentMaximum,
    max: percentMaximum,
    detail: '4 KB'
  }
]

const activitySamples: Omit<ActivityEntry, 'time'>[] = [
  {
    id: 'budget-error',
    title: 'File could not be read',
    detail: 'Budget.xlsx is open in another application. Close the file to allow DRIP to read it. Sample error: access denied (EACCES).',
    severity: 'error',
    path: 'Documents / Personal / Budget.xlsx'
  },
  { id: 'upload', title: 'Upload started', detail: 'Changed chunks are being sent to the server.', severity: 'info', path: 'Photos / Ireland / Coastline.mp4' },
  {
    id: 'hash',
    title: 'File hashing started',
    detail: 'Preparing a fingerprint for the changed file.',
    severity: 'info',
    path: 'Photos / Ireland / Morning light.jpg'
  },
  {
    id: 'scan-start',
    title: 'Chunk scan started',
    detail: 'Checking the file for changed chunks.',
    severity: 'info',
    path: 'Projects / Studio / Design archive.zip'
  },
  {
    id: 'synced',
    title: 'File synced',
    detail: 'All changed chunks have reached the server.',
    severity: 'success',
    path: 'Documents / Studio / Project brief.pdf'
  },
  {
    id: 'scan-end',
    title: 'Chunk scan completed',
    detail: 'The file is ready to upload.',
    severity: 'success',
    path: 'Documents / Studio / Project brief.pdf'
  },
  { id: 'changed', title: 'File change detected', detail: 'The watched file was modified.', severity: 'info', path: 'Documents / Studio / Project brief.pdf' },
  { id: 'connected', title: 'Connected to server', detail: 'A connection to sync.drip.example was established.', severity: 'success' }
]
export const sampleActivity: ActivityEntry[] = activitySamples.map((entry, index) => ({
  ...entry,
  time: new Date(recent.getTime() - index * minute).toISOString()
}))

import type { HeadsPage } from '../../protocol/sync'
import type { SyncTransport } from './http'
import type { SyncStore } from './store'
import type { SyncCredentials, SyncRoot } from './types'

export async function reconcileRoot(store: SyncStore, transport: SyncTransport, credentials: SyncCredentials, root: SyncRoot, active: () => boolean) {
  const heads: HeadsPage['heads'] = []
  let after: string | null = null
  do {
    const query: string = `?revision=${root.revision}&limit=100${after ? '&after=' + encodeURIComponent(after) : ''}`
    const page: HeadsPage = await transport.request<HeadsPage>(credentials, `/v1/roots/${root.rootId}/heads${query}`)
    if (page.revision !== root.revision || !Array.isArray(page.heads) || (page.next !== null && typeof page.next !== 'string'))
      throw new Error('Server heads changed during reconciliation.')
    heads.push(...page.heads)
    if (after !== null && page.next === after) throw new Error('Server heads pagination did not advance.')
    after = page.next
  } while (after !== null && active())
  if (active() && store.readyScopes().length) store.reconcile(root, heads)
}

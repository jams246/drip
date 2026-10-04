export class RootLocks {
  private readonly waiting = new Map<string, Promise<void>>()

  async run<T>(rootId: string, operation: () => Promise<T>): Promise<T> {
    const previous = this.waiting.get(rootId) ?? Promise.resolve()
    const result = previous.then(operation)
    const settled = result.then(
      () => undefined,
      () => undefined
    )
    this.waiting.set(rootId, settled)
    try {
      return await result
    } finally {
      if (this.waiting.get(rootId) === settled) this.waiting.delete(rootId)
    }
  }

  async settled() {
    await Promise.all(this.waiting.values())
  }
}

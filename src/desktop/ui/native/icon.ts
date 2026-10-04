declare function js_drip_set_window_icon(): void
declare function js_drip_watch_start(path: string, recursive: boolean): number
declare function js_drip_watch_stop(id: number): void
declare function js_drip_watch_drain(): string
declare function js_drip_watch_ack(id: number, sequence: number): void
declare function js_drip_watch_shutdown(): void
declare function js_drip_query_path(path: string): string
declare function js_drip_query_names(path: string): string
declare function js_drip_shell_init(): boolean
declare function js_drip_shell_drain(): string
declare function js_drip_shell_hide(): void
declare function js_drip_shell_open(): void
declare function js_drip_shell_paused(paused: boolean): void
declare function js_drip_shell_exit(): void
declare function js_drip_instance_claim(): boolean
declare function js_drip_instance_release(): void
declare function js_drip_diagnostics_init(): boolean
declare function js_drip_diagnostics_write(message: string): void

export function setWindowIcon() {
  js_drip_set_window_icon()
}

export function watchStart(path: string, recursive: boolean) {
  return js_drip_watch_start(path, recursive)
}
export function watchStop(id: number) {
  js_drip_watch_stop(id)
}
export function watchDrain() {
  return js_drip_watch_drain()
}
export function watchAck(id: number, sequence: number) {
  js_drip_watch_ack(id, sequence)
}
export function watchShutdown() {
  js_drip_watch_shutdown()
}
export function queryPath(path: string) {
  return js_drip_query_path(path)
}
export function queryNames(path: string) {
  return js_drip_query_names(path)
}
export function shellInit() {
  return js_drip_shell_init()
}
export function shellDrain() {
  return js_drip_shell_drain()
}
export function shellHide() {
  js_drip_shell_hide()
}
export function shellOpen() {
  js_drip_shell_open()
}
export function shellPaused(paused: boolean) {
  js_drip_shell_paused(paused)
}
export function shellExit() {
  js_drip_shell_exit()
}
export function claimInstance() {
  return js_drip_instance_claim()
}
export function releaseInstance() {
  js_drip_instance_release()
}
export function initializeDiagnostics() {
  return js_drip_diagnostics_init()
}
export function writeDiagnostic(message: string) {
  js_drip_diagnostics_write(message)
}

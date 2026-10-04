/* oxlint-disable eslint/no-magic-numbers -- UUID version/variant bits and hexadecimal byte layout. */
export function randomHex(bytes = 32): string {
  const value = new Uint8Array(bytes)
  crypto.getRandomValues(value)
  let result = ''
  for (let index = 0; index < value.length; index++) result += value[index].toString(16).padStart(2, '0')
  return result
}

export function randomId(): string {
  const value = randomHex(16)
  const variant = ((parseInt(value[16], 16) & 3) | 8).toString(16)
  return `${value.slice(0, 8)}-${value.slice(8, 12)}-4${value.slice(13, 16)}-${variant}${value.slice(17, 20)}-${value.slice(20)}`
}

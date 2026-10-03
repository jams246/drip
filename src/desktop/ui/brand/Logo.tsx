export function Logo() {
  return (
    <svg className="brand__logo" viewBox="0 0 40 48" aria-hidden="true">
      <path fill="currentColor" d="M20 2C15 10 4 21 4 30a16 16 0 0 0 32 0C36 21 25 10 20 2Z" />
      <path
        className="brand__arrow"
        d="M20 35V20m-6 6 6-6 6 6"
        fill="none"
        stroke="currentColor"
        strokeWidth="3.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

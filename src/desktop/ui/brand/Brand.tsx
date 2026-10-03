import { Logo } from './Logo'
import './brand.css'

export function Brand() {
  return (
    <div className="brand">
      <Logo />
      <div className="brand__copy">
        <span className="brand__name">DRIP</span>
        <span className="brand__meaning">
          <span>
            <span className="brand__initial">D</span>irected
          </span>{' '}
          <span>
            <span className="brand__initial">R</span>emote
          </span>{' '}
          <span>
            <span className="brand__initial">I</span>ncremental
          </span>{' '}
          <span>
            <span className="brand__initial">P</span>ush
          </span>
        </span>
      </div>
    </div>
  )
}

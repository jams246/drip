import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { DesktopApp } from './shell/DesktopApp'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <DesktopApp />
  </StrictMode>
)

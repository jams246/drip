import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { DesktopApp } from './shell/DesktopApp'
import { sampleFiles } from './sync/samples'
import { useSampleProgress } from './sync/useSampleProgress'

function App() {
  const [files, setFiles] = useState(sampleFiles)
  const [paused, setPaused] = useState(false)
  useSampleProgress(paused, setFiles)
  return <DesktopApp files={files} onFilesChange={setFiles} paused={paused} onPauseChange={setPaused} />
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
)

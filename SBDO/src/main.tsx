import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { PreferencesProvider } from './settings/PreferencesContext'
import { ConfirmProvider } from './ui/Modal'
import {
  applyResolvedTheme,
  loadPreferences,
  resolveTheme,
} from './settings/preferences'
import './styles/tokens.css'
import './index.css'

const initial = loadPreferences()
applyResolvedTheme(resolveTheme(initial.theme))
document.documentElement.lang = initial.locale

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PreferencesProvider>
      <ConfirmProvider>
        <App />
      </ConfirmProvider>
    </PreferencesProvider>
  </StrictMode>,
)

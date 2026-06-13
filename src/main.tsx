import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { BrowserRouter } from 'react-router-dom'
import App from './App'
import { LanguageProvider } from './contexts/LanguageContext'
import { ShortcutsProvider } from './contexts/ShortcutsContext'
import { ThemeProvider } from './contexts/ThemeContext'
import { applyTheme, loadTheme } from './lib/theme'
import './index.css'

// Apply saved theme before first paint to avoid a flash of default colors.
applyTheme(loadTheme())

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <LanguageProvider>
      <ThemeProvider>
        <ShortcutsProvider>
          <QueryClientProvider client={queryClient}>
            <BrowserRouter>
              <App />
            </BrowserRouter>
          </QueryClientProvider>
        </ShortcutsProvider>
      </ThemeProvider>
    </LanguageProvider>
  </StrictMode>,
)
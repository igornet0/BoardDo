import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { extensions, type ExtensionStatus } from '../api/projecthub'

interface ExtensionsValue {
  projecthub: ExtensionStatus | null
  /** Installed, self-tested and switched on — ProjectHub actions are available. */
  projecthubReady: boolean
  refresh: () => Promise<void>
  setProjecthub: (status: ExtensionStatus) => void
}

const Ctx = createContext<ExtensionsValue>({
  projecthub: null,
  projecthubReady: false,
  refresh: async () => {},
  setProjecthub: () => {},
})

export function ExtensionsProvider({ children }: { children: ReactNode }) {
  const [projecthub, setProjecthub] = useState<ExtensionStatus | null>(null)

  const refresh = useCallback(async () => {
    try {
      setProjecthub(await extensions.status())
    } catch {
      setProjecthub(null)
    }
  }, [])

  useEffect(() => {
    void refresh()
    const id = window.setInterval(() => void refresh(), 30_000)
    return () => window.clearInterval(id)
  }, [refresh])

  const value = useMemo<ExtensionsValue>(
    () => ({
      projecthub,
      projecthubReady: Boolean(projecthub?.installed && projecthub.enabled),
      refresh,
      setProjecthub,
    }),
    [projecthub, refresh],
  )
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}

export function useExtensions(): ExtensionsValue {
  return useContext(Ctx)
}

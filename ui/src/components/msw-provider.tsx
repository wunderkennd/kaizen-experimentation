'use client';

import { useEffect, useState } from 'react';

const MOCK_ENABLED = process.env.NEXT_PUBLIC_MOCK_API === 'true';

/**
 * When NEXT_PUBLIC_MOCK_API=true, intercepts all fetch calls with MSW
 * mock handlers so the UI can run without any backend services.
 * When false (or unset), renders children immediately — real API
 * calls go through Next.js rewrites proxy to backend services.
 */
export function MswProvider({ children }: { children: React.ReactNode }) {
  const [ready, setReady] = useState(!MOCK_ENABLED);

  useEffect(() => {
    if (!MOCK_ENABLED) return;

    import('@/__mocks__/browser').then(({ worker }) => {
      worker.start({ onUnhandledRequest: 'bypass' }).then(() => {
        setReady(true);
      });
    });
  }, []);

  if (!ready) {
    return null;
  }

  return <>{children}</>;
}

'use client';

import { useEffect, useState } from 'react';

export function MswProvider({ children }: { children: React.ReactNode }) {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (process.env.NODE_ENV !== 'development') {
      setReady(true);
      return;
    }

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

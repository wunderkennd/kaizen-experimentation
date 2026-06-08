import './globals.css';
import { NavHeader } from '@/components/nav-header';
import { MswProvider } from '@/components/msw-provider';
import { AuthProvider } from '@/lib/auth-context';
import { ErrorBoundary } from '@/components/error-boundary';
import { ToastProvider } from '@/lib/toast-context';
import { ToastContainer } from '@/components/toast-container';

export const metadata = {
  title: 'Experimentation Platform',
  description: 'Decision Support Dashboard',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen bg-gray-50 text-gray-900 antialiased">
        <a
          href="#main-content"
          className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:z-50 focus:rounded-md focus:bg-indigo-600 focus:px-4 focus:py-2 focus:text-white focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:ring-offset-2"
        >
          Skip to main content
        </a>
        <MswProvider>
          <AuthProvider>
            <ToastProvider>
              <NavHeader />
              <main
                id="main-content"
                tabIndex={-1}
                className="mx-auto max-w-7xl px-4 py-6 sm:px-6 lg:px-8 focus:outline-none"
              >
                <ErrorBoundary>
                  {children}
                </ErrorBoundary>
              </main>
              <ToastContainer />
            </ToastProvider>
          </AuthProvider>
        </MswProvider>
      </body>
    </html>
  );
}

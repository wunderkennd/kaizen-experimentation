import './globals.css';
import { NavHeader } from '@/components/nav-header';
import { MswProvider } from '@/components/msw-provider';

export const metadata = {
  title: 'Experimentation Platform',
  description: 'Decision Support Dashboard',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen bg-gray-50 text-gray-900 antialiased">
        <MswProvider>
          <NavHeader />
          <main className="mx-auto max-w-7xl px-4 py-6 sm:px-6 lg:px-8">
            {children}
          </main>
        </MswProvider>
      </body>
    </html>
  );
}

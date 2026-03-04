export const metadata = {
  title: 'Experimentation Platform',
  description: 'Decision Support Dashboard',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}

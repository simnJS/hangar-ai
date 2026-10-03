import type { Metadata } from "next";

import "./globals.css";

export const metadata: Metadata = {
  title: "Hangar Cloud",
  description: "Hosted shared board for Hangar.AI teams and their agents.",
};

/**
 * No `<ClerkProvider>` here on purpose.
 *
 * The only page this app renders is static, and wrapping it in a Clerk
 * provider would drag the publishable key into prerendering — meaning
 * `next build` would need an environment. The dashboard, when it lands, brings
 * its own provider in its own segment layout.
 */
export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}

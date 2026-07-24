## 2025-05-14 - Navigation Active States
**Learning:** Users often lose track of their location in single-page applications. Providing clear visual and screen-reader cues for the current route is essential for spatial awareness.
**Action:** Use `aria-current="page"` and a distinct primary color for active navigation links.
## 2026-03-17 - Copy-to-Clipboard for Code Blocks
**Learning:** Adding a "Copy" button to syntax-highlighted code blocks significantly improves the developer UX when interacting with metric definitions or SQL logs. Using `next/dynamic` with `ssr: false` and a pulse loading state prevents hydration errors and improves perceived performance for components using heavy libraries like Prism.
**Action:** Always include a copy-to-clipboard utility for technical snippets. Ensure the button is discoverable via hover and keyboard focus (`focus:block`), and provide immediate visual feedback ("Copied!") to confirm success.
## 2026-07-21 - Standardizing Table Header Focus Rings
**Learning:** Interactive headers (like those used for column sorting) can easily be overlooked in keyboard navigation design. While utilizing standard `aria-sort` attributes communicates current states, a visually distinct focus ring is crucial to ensure keyboard-only users can clearly identify which header is focused. Inset rings without offsets often overlap column text or visual sorting indicators (like chevrons).
**Action:** Standardize on `focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2` for all interactive table headers, ensuring a clear gap between the header button and the focus ring.

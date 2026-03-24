## 2025-05-14 - Navigation Active States
**Learning:** Users often lose track of their location in single-page applications. Providing clear visual and screen-reader cues for the current route is essential for spatial awareness.
**Action:** Use `aria-current="page"` and a distinct primary color for active navigation links.
## 2026-03-17 - Copy-to-Clipboard for Code Blocks
**Learning:** Adding a "Copy" button to syntax-highlighted code blocks significantly improves the developer UX when interacting with metric definitions or SQL logs. Using `next/dynamic` with `ssr: false` and a pulse loading state prevents hydration errors and improves perceived performance for components using heavy libraries like Prism.
**Action:** Always include a copy-to-clipboard utility for technical snippets. Ensure the button is discoverable via hover and keyboard focus (`focus:block`), and provide immediate visual feedback ("Copied!") to confirm success.

## 2025-05-20 - Reusable Copy Components
**Learning:** Consolidating micro-interactions like copy-to-clipboard into reusable components ensures consistent ARIA labels, focus states, and feedback mechanisms (toasts) across the application. Technical identifiers (IDs) are prime candidates for this pattern to improve developer UX.
**Action:** Use the `CopyButton` component for all technical identifiers. Ensure it is placed consistently (e.g., next to the ID or in a metadata grid) and provides immediate toast feedback.

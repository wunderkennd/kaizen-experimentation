## 2025-05-14 - Navigation Active States
**Learning:** Users often lose track of their location in single-page applications. Providing clear visual and screen-reader cues for the current route is essential for spatial awareness.
**Action:** Use `aria-current="page"` and a distinct primary color for active navigation links.
## 2026-03-17 - Copy-to-Clipboard for Code Blocks
**Learning:** Adding a "Copy" button to syntax-highlighted code blocks significantly improves the developer UX when interacting with metric definitions or SQL logs. Using `next/dynamic` with `ssr: false` and a pulse loading state prevents hydration errors and improves perceived performance for components using heavy libraries like Prism.
**Action:** Always include a copy-to-clipboard utility for technical snippets. Ensure the button is discoverable via hover and keyboard focus (`focus:block`), and provide immediate visual feedback ("Copied!") to confirm success.
## 2026-06-12 - Reusable Copy Components for IDs
**Learning:** Standardizing copy-to-clipboard behavior for technical identifiers (Experiment/Flag/Metric IDs) across the platform creates a consistent micro-UX pattern. Using a centralized component ensures all IDs have accessible ARIA labels, hover-discoverability, and immediate toast feedback, which users quickly learn to rely on for quick data extraction.
**Action:** Use the `CopyButton` component for all user-facing technical identifiers. Ensure it uses `e.stopPropagation()` when used inside interactive rows to avoid conflicting with expansion or navigation.

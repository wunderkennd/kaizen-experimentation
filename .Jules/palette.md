# Palette's Journal

Critical UX and accessibility learnings from the Experimentation Platform.

## 2025-05-14 - Navigation Active States
**Learning:** Users often lose track of their location in single-page applications. Providing clear visual and screen-reader cues for the current route is essential for spatial awareness.
**Action:** Use `aria-current="page"` and a distinct primary color for active navigation links.
## 2025-05-15 - Async Feedback in Modals
**Learning:** Users often click primary buttons in modals multiple times if there's no immediate visual feedback during async transitions. Even with a "Processing..." text change, a spinning icon provides a more standard and expected "active" state.
**Action:** Always include a loading spinner in modal primary buttons that trigger network requests or complex state transitions.
## 2026-03-17 - Copy-to-Clipboard for Code Blocks
**Learning:** Adding a "Copy" button to syntax-highlighted code blocks significantly improves the developer UX when interacting with metric definitions or SQL logs. Using `next/dynamic` with `ssr: false` and a pulse loading state prevents hydration errors and improves perceived performance for components using heavy libraries like Prism.
**Action:** Always include a copy-to-clipboard utility for technical snippets. Ensure the button is discoverable via hover and keyboard focus (`focus:block`), and provide immediate visual feedback ("Copied!") to confirm success.
## 2025-05-15 - [Distribute Evenly UX Win]
**Learning:** Automating repetitive calculations (like equal traffic distribution) significantly reduces friction and errors in experiment setup. Users appreciate tools that handle precision and edge cases (like remainder distribution) for them.
**Action:** Look for other manual calculation tasks in the UI, such as target sample size or duration estimation, and provide one-click solutions.

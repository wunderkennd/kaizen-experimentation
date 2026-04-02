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
## 2026-03-31 - Automating Traffic Distribution in Wizard
**Learning:** Manual traffic distribution in multi-variant experiments is error-prone and tedious for users. Automating this with a "Distribute Evenly" button that handles rounding remainders (e.g., 33.3%, 33.3%, 33.4%) significantly improves the experiment setup UX.
**Action:** Implement "Distribute Evenly" for any multi-input resource allocation UI. Ensure it is accessible via keyboard and provides immediate visual feedback on the total sum.
## 2026-03-24 - Standardizing Clipboard Interactions
**Learning:** A reusable `CopyButton` component provides a consistent and accessible way for users to copy technical identifiers. Standardizing this pattern across Experiment IDs, Flag IDs, and SQL blocks improves the overall discoverability and reliability of the feature.
**Action:** Use the `CopyButton` component for all technical identifiers. Ensure it includes an `aria-label` for screen readers and provides both visual (icon change) and toast feedback. Wrap tests for pages using this component in `ToastProvider` to avoid context errors.

## 2026-04-05 - Copy-to-Clipboard in Wizard Review
**Learning:** Technical identifiers (like Layer IDs or primary metrics) in the final review step of a creation wizard are high-value copy targets for users who need to document the setup before clicking "Create". When placing these buttons in dense description lists, ensuring vertical alignment with `flex items-center gap-2` maintains visual hierarchy without disrupting the layout.
**Action:** Extend description list helpers (like `DlRow`) to support optional copy functionality for identifiers. Always use a smaller button size (e.g., `h-4 w-4`) to fit within single-line list items.

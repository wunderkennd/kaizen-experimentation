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

## 2026-04-10 - Enhancing Review Steps with Copy-to-Clipboard
**Learning:** Providing technical identifiers in a `<code>` block with an adjacent `CopyButton` in a "Review" or "Summary" step significantly reduces the cognitive load for users who need to verify or cross-reference these IDs before final submission. Using `text-xs` for these inline code blocks maintains legibility without breaking the layout of dense description lists.
**Action:** Always include copy-to-clipboard utilities for primary technical IDs in summary views. Use `flex items-center gap-2` to align the code block and the copy button.

## 2026-05-20 - Keyboard Accessibility for Expandable Rows
**Learning:** Making table rows clickable for expansion is a common UX pattern, but it's completely inaccessible to keyboard users unless explicitly handled. Adding `role="button"`, `tabIndex={0}`, and an `onKeyDown` handler for 'Enter' and 'Space' keys is critical for a11y.
**Action:** When making non-interactive elements (like table rows) interactive, always provide standard keyboard triggers and appropriate ARIA roles/attributes.

## 2026-05-20 - Consistent Search UI Pattern
**Learning:** Consistency in repetitive UI elements like search bars builds user trust and makes the interface feel predictable. Using a relative container with an absolute-positioned icon (with `pointer-events-none`) and `pl-9` padding on the input is the established pattern in this app.
**Action:** Always wrap search inputs in a relative container with a magnifying glass SVG to maintain visual consistency with other filtered lists.

## 2026-04-12 - Expandable Audit Log for Technical IDs
**Learning:** Even if an audit entry doesn't have "changed values", users still benefit from row expansion to access the Experiment ID and other metadata without leaving the page.
**Action:** Design tables such that all rows are interactive/expandable if they contain hidden technical identifiers that are useful for developer workflows.

## 2026-04-15 - Keyboard Accessibility for Expandable Rows
**Learning:** Interactive table rows that toggle visibility of details must be explicitly marked as buttons and support keyboard navigation. Without `role="button"` and `onKeyDown` handlers for Enter/Space, these features remain "mouse-only" and inaccessible to screen reader or keyboard-only users.
**Action:** Always add `role="button"`, `tabIndex={0}`, `aria-expanded`, and keyboard handlers to custom interactive containers that lack native button semantics. Use `focus-within` with a ring to provide clear focus indicators.

## 2026-05-21 - Keyboard Accessibility for Sortable Headers
**Learning:** Table headers used for sorting must be semantic and keyboard-accessible. Moving click handlers from the `<th>` to an internal `<button>` ensures they are in the tab order and provide standard focus indicators.
**Action:** Always wrap sortable header content in a `<button>` within the `<th>`. Use `aria-sort` on the `<th>` to communicate state, and ensure decorative sort icons are hidden from screen readers.

## 2026-05-22 - Standardized Search UI Pattern
**Learning:** Using a consistent visual pattern for search inputs (magnifying glass icon + inset text) creates a predictable experience for users scanning filtered lists. Achieving precise vertical alignment between the absolute-positioned icon and the input text requires consistent vertical padding (e.g., `py-1.5` or `py-2`) depending on the line height.
**Action:** Always wrap search inputs in a `relative` container with an absolute-positioned magnifying glass SVG. Use `pl-9` to clear the icon and ensure `pointer-events-none` on the icon to avoid interfering with input focus.

## 2026-05-23 - Reusable Search Clearing Pattern
**Learning:** Providing an explicit way to clear filters in both the toolbar and the empty state significantly reduces interaction friction. Consistency in styling these "Clear filters" buttons (e.g., subtle gray border for toolbar, indigo text for empty state) helps users quickly identify recovery actions across different list views.
**Action:** Always include "Clear filters" buttons in search-enabled lists. Use `rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-600 hover:bg-gray-50` for toolbars and `mt-2 text-sm text-indigo-600 hover:text-indigo-800` for empty state messages.

## 2026-06-15 - Actionable Empty States
**Learning:** A blank screen or a simple "No items found" message can be a dead-end for users. Providing a direct "Call to Action" (CTA) link in empty states reduces friction and guides users toward the next logical step in their workflow, provided they have the necessary permissions.
**Action:** Always include a primary action link or button in empty state components for list pages. Ensure the visibility of this CTA is gated by appropriate user permissions to maintain RBAC consistency.

## 2026-05-25 - Standardized Search and Empty States
**Learning:** Consistency in search UI (icon positioning and padding) and actionable empty states (CTAs and clear-filters) significantly reduces user friction when navigating large datasets like feature flags or audit logs. Decorative icons should always be hidden from screen readers using `aria-hidden="true"`.
**Action:** Apply the `relative` container with `pl-9` padding and `aria-hidden` SVG pattern to all search inputs. Always provide a way to reset filters in empty result states.

## 2026-04-20 - Loading State for Feature Flag Promotion
**Learning:** Promoting a feature flag to an experiment involves a network request and a navigation transition. Providing a loading spinner in the "Promote" button and disabling it during the process prevents duplicate submissions and gives clear feedback to the user.
**Action:** Always implement a loading state (spinner + disabled state) for primary action buttons that trigger resource promotion or major state transitions outside of modals.

## 2026-06-20 - Loading Feedback for Resource Updates
**Learning:** For forms that update resources (like Edit Flag), providing a loading spinner in the "Save" button is as important as in creation or promotion flows. It signals that the system is processing the update and prevents redundant save attempts during network latency.
**Action:** Always include an `animate-spin` SVG and a "Saving..." state in the submit button of resource edit forms.

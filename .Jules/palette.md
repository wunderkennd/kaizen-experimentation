# Palette's Journal

Critical UX and accessibility learnings from the Experimentation Platform.

## 2026-07-10 - Unified Keyboard Focus Indicators for Utility Components
**Learning:** Utility buttons like `CopyButton` are highly interactive and frequent across dense layouts. Using standard focus indicators (`focus:outline-none focus:ring-2`) can cause confusing ring highlights during mouse clicks, while completely ignoring them is an accessibility blocker. Applying the `focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2` pattern ensures keyboard navigators receive standard, clear spatial visual cues, while keeping the interface pristine and ring-free during normal mouse usage.
**Action:** Always utilize `focus-visible` states rather than standard `focus` rings on discrete utility buttons to ensure accessibility without introducing visual noise on click interactions.

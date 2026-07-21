## 2026-07-06 - Standardizing Table Header Focus Rings

**Learning:** Interactive headers (like those used for column sorting) can easily be overlooked in keyboard navigation design. While utilizing standard `aria-sort` attributes communicates current states, a visually distinct focus ring is crucial to ensure keyboard-only users can clearly identify which header is focused. Inset rings without offsets often overlap column text or visual sorting indicators (like chevrons).

**Action:** Standardize on `focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2` for all interactive table headers, ensuring a clear gap between the header button and the focus ring.

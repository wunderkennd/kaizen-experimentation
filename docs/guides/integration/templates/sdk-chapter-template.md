<!--
SDK Chapter Template — Wave 2 agents fill this in, one file per SDK.

Usage:
1. Copy this file to `docs/guides/integration/06-sdks/0N-<sdk>.md`.
2. Replace `<SDK NAME>` and all placeholder text.
3. Keep ALL section headings in ALL files identical so customers can diff SDKs side-by-side.
4. The HTML comment under each heading is your "what goes here" instruction — delete it
   once you have written the section.
5. Every code block must reference a runnable example under `examples/<sdk>/` — create the
   example file if it does not exist, and cite it with a `// examples/<sdk>/<file>` header comment.
6. Every API call you mention must correspond to a real RPC. If you cannot verify the RPC name
   in `proto/experimentation/`, use a `<!-- TODO(wave-2): confirm RPC name -->` comment and move on.
7. Do NOT remove the "What you'll learn" block at the top or the "Next steps" block at the bottom.
-->

# 6.N <SDK NAME> SDK

> **What you'll learn**
> - How to install, configure, and initialize the <SDK NAME> SDK
> - How to fetch an assignment, emit an exposure, and evaluate a flag from your runtime
> - How the SDK behaves offline, under load, and during a Kaizen outage

<!--
Open with 2–3 sentences: who this SDK is for (platform, runtime version), what language idioms
it follows, and the one "gotcha" unique to this SDK (e.g., "Web has SSR/CSR split;" "iOS has a
cold-start budget;" "Go relies on context propagation").
-->

## Install

<!--
What goes here: the exact installation command for the package registry used by this SDK
(npm/yarn/pnpm for Web; Swift Package Manager for iOS; Gradle/Maven for Android; `go get`
for Go; pip/poetry/uv for Python). Include minimum runtime version and any platform-specific
prerequisites. Cite the actual package path — do not guess.
-->

## Initialize

<!--
What goes here: a complete initialization snippet showing how to create the SDK client with
an API key / service account, set the environment (dev/staging/prod), and configure timeouts.
Point the reader at `examples/<sdk>/initialize.*` for a runnable version.
-->

## Fetch Assignment

<!--
What goes here: how to call GetAssignment (and GetAssignments batch) from this SDK. Show the
assignment unit being passed correctly per §2.1.2, how to read the returned variant payload,
and how to deal with experiments the SDK has not cached yet. Reference the actual SDK method
name verified against `sdks/<sdk>/` source.
-->

## Emit Exposure

<!--
What goes here: how to emit an ExposureEvent correctly — at render time, not at assignment
time (see §2.4). Cover SDK-managed batching, immediate-mode for testing, and the contract
with M2 Pipeline (port 50052). Show how a trigger-event pattern is expressed in this SDK.
-->

## Evaluate Flag

<!--
What goes here: how to call EvaluateFlag / EvaluateFlags against M7 (port 50057), including
how to pass targeting attributes, how to read the returned typed value (bool/string/JSON/number),
and how to handle "flag not found" vs. "flag evaluated to default." Reference §2.1.4 for the
flag-vs-experiment distinction.
-->

## Handle Offline / Fallback

<!--
What goes here: the SDK's behavior when M1, M2, or M7 is unreachable. Cover: last-known-variant
caching, default-variant behavior, local event buffering, retry/backoff schedule, cache TTL,
and how to disable the cache for testing. Cross-link to Chapter 14 for the full DR story.
-->

## Shutdown

<!--
What goes here: how to flush buffered events and close network connections cleanly on process
exit. Language-specific: e.g., Web `beforeunload`, iOS `applicationWillTerminate`, Android
lifecycle, Go `defer client.Close()`, Python context manager. Include the timeout semantics —
how long `Shutdown` will block waiting for pending events.
-->

## Error Handling

<!--
What goes here: a table of error categories (NETWORK, CONFIG_STALE, PERMISSION_DENIED, etc.)
with the recommended customer response. Distinguish errors that mean "retry" from errors that
mean "fix your config" from errors that mean "ignore and proceed with default." Reference the
error catalog at `docs/guides/integration/16-reference/error-codes.md` once available.
-->

## Observability Hooks

<!--
What goes here: how the SDK emits OpenTelemetry spans, which span names and attributes to
expect, and how to plug in a custom logger/metrics sink. Show a snippet of what a typical
trace looks like. Note any cardinality warnings — e.g., do NOT tag spans with user_id.
-->

## Troubleshooting

<!--
What goes here: three to five of the most common failure modes customers hit when integrating
this SDK, each with a symptom, diagnosis, and fix. Typical entries: "I'm getting the default
variant always," "My exposures aren't showing up in the dashboard," "SDK initialization hangs
on app launch." Link to Chapter 18 FAQ for broader cross-SDK issues.
-->

## Next steps

<!--
Replace these placeholders with real links. Every SDK chapter must end with a "Next steps"
block linking to: (a) the feature chapter most relevant to this SDK's runtime, and
(b) the quickstart cookbook recipe that uses this SDK.
-->

- Continue to [Chapter 7 — Creating and Managing Experiments](../07-experiments.md) to create your first experiment and point this SDK at it.
- For a runnable end-to-end recipe using this SDK, see [Cookbook recipe TBD](../17-cookbook/README.md).

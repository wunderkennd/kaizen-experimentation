# ADR-008: Auto-Pause as Default Guardrail Behavior

## Status
Accepted

## Date
2026-03-03

## Context
Spotify developers roll back approximately 42% of experiments to prevent business metric regressions. This indicates that guardrail breaches are common and that fast automated response is critical. The question is whether guardrail breaches should alert (human decides) or auto-pause (machine acts, human overrides).

## Decision
Guardrail breaches auto-pause experiments by default:

1. M3 detects breach during hourly guardrail computation.
2. M3 publishes `GuardrailAlert` to `guardrail_alerts` Kafka topic.
3. M5 consumes alert within 60 seconds and transitions experiment to paused state (traffic allocation → 0%).
4. Experiment owner receives Slack + PagerDuty notification with breach details.
5. To continue despite a breach, the owner must explicitly set `guardrail_action: ALERT_ONLY` — an audited action logged in the experiment audit trail.

## Alternatives Considered
- **Alert-only as default**: Less disruptive, but if the default requires human action, breaches on Friday afternoon go unaddressed until Monday. The safe default protects the platform's credibility: passing guardrails means something.
- **Auto-conclude (not just pause)**: Too aggressive. A transient spike in rebuffer rate due to a CDN incident shouldn't permanently conclude an experiment. Pause preserves the option to resume.
- **Graduated response (warn → pause → conclude)**: Reasonable, but adds complexity. The `consecutive_breaches_required` field on GuardrailConfig already provides graduated sensitivity. Auto-pause on breach count ≥ threshold is simpler.

## Consequences
- PMs must understand that guardrails are enforced, not advisory, by default.
- The ALERT_ONLY override creates an audit trail — useful for post-mortems and governance.
- False positive guardrail breaches will pause experiments unnecessarily. Guardrail thresholds must be set thoughtfully; M5 should provide guidance on threshold selection.

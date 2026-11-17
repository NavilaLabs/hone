---
id: dec-7k3m2q8p
type: decision
ticket: tk-9f2x4a8m
status: accepted
created: 2026-11-16
authors: [alice]
reach: cross-cutting
links:
  supersedes: [dec-3b8n1c5w]
  affects: [mod-5w7p1r2k]
  code: ["src/payments/gateway.rs#PaymentGateway"]
review:
  approved_by:
    - { name: alice, role: architect, hash: sha256:6707fdc5fdd423057322a6f4d934968ab60848484affd95ee104d31c38905d6e, date: 2026-11-16 }
  summary:
    - q: "What happens when the queue fails?"
      a: "Fall back to synchronous processing, see dec-4q8r2n6h."
    - rejected: "Outbox pattern: too much operational effort for this ticket."
---
# Process payments asynchronously

## Context and problem

Synchronous processing blocks the checkout while the payment provider responds.

## Options

1. Keep it synchronous and lower the timeouts (the naïve approach).
2. A queue with idempotent workers.
3. Outbox pattern.

## Decision

Option 2. The checkout puts an order into the queue and responds immediately.

## Consequences

- The interface of `PaymentGateway` changes: the result arrives as an event.
- A queue failure needs a fallback, see `dec-4q8r2n6h`.

---
authors:
  - "alice"
created: "2026-11-16"
id: 'dec-7k3m2q8p'
links:
  code:
    - src/payments/gateway.rs#PaymentGateway
  affects:
    - mod-5w7p1r2k
  supersedes: ["dec-3b8n1c5w"]
reach: "cross-cutting"
review:
  approved_by: []
status: "accepted"
ticket: tk-9f2x4a8m
type: 'decision'
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

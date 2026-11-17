---
id: tp-8w3j6q1z
type: test-plan
ticket: tk-9f2x4a8m
status: proposed
created: 2026-11-17
authors: [alice]
links:
  verifies: [spec-6d1f8b2e]
  code: ["tests/payments/async_test.rs#checkout_returns_immediately"]
  transcript: ["https://example.invalid/sessions/42"]
---
# Test plan for asynchronous payments

- The checkout responds without waiting for the payment provider.

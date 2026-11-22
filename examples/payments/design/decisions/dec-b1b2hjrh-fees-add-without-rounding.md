---
id: "dec-b1b2hjrh"
type: "decision"
ticket: "tk-y4075sb1"
status: "accepted"
created: "2026-11-22"
authors: ["alice"]
links:
  affects: ["mod-f8mk9hpn"]
  relates: ["spec-99wa1at4"]
review:
  approved_by:
    - { name: "alice", role: "architect", hash: "sha256:eb41dd8f4dcf050781f8a0b831c820902b882ff7808814e6158c02e5ce9ca9a2", date: "2026-11-22" }
---
# Fees add without rounding

`Fee::add` sums whole cents and never rounds, because every provider already charges whole
cents. A provider with fractional fees would need a new decision.

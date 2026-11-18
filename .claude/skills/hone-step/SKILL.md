---
name: hone-step
description: Run one step of hone's concept Section 16 (for example "step 9: replication") from design to commits. Use when a task or handover names a step of Section 16, or asks to decide and build a part of the protocol.
---

# hone step

A step turns open parts of `docs/protocol-concept.md` into decided text and working code. The rules in `CLAUDE.md` apply throughout; this skill gives the order.

## 1. Read

Read the concept sections the step touches, the related Section 15 questions, and the code paths involved. Done when you can name, for the step, every data format, command, and error it adds.

## 2. Smoke-test dependencies

For every new dependency or runtime behaviour the step relies on, write a tiny program in the scratchpad and run it through the real binary: errors, limits, timeouts, what happens inside a tokio runtime, what leaks to stdout. Done when every behaviour the design will assume has been observed, not read.

## 3. Grill

List every rule the concept leaves open, including small ones (an empty input, a missing file, a torn line, a malformed config). Ask them with AskUserQuestion in rounds of at most four, numbered. Each question:

- names one concrete situation in plain language, as an example ("ticket tk-9f2x4a8m was started with plugin 1.0, now 1.1 lies there"),
- puts the recommended option first, marked "(Recommended)",
- says what an existing term was meant for when an option stretches it, and recommends the clean split,
- points out fields that another field already implies, and recommends dropping them.

When the user answers with a counter-question, answer it honestly with evidence (run a check if needed), then ask again. Done when every listed rule has an answer.

## 4. Write the concept

Write the decisions into the concept: the normative sections, Section 12 for tool-level choices, and Section 15 as the next numbered question with a one-paragraph summary. Run Prettier and commit as `docs(concept): …`. Done when the concept answers every rule from step 3.

## 5. Build

Implement in `hone-core` behind a narrow interface, then the thin adapters in `hone-cli` and `hone-mcp`. Write the test cases as the concept's test-case format prescribes (Section 14). Whenever building reveals a rule the concept does not state, note it for the report rather than deciding it silently; ask if it changes behaviour others rely on. Done when the four checks of `CLAUDE.md` pass and every negative and positive case behaves as `CLAUDE.md` "Tests and fixtures" requires.

## 6. Commit and report

Split into feat, test, and docs commits, each passing on its own; mark the step as done in Section 16 in the docs commit. Report to the user: the commits, what the tests cover, and every rule you decided while building, numbered, for confirmation. Then offer a handover with the skill `handover`.

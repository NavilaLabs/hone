# Contributing to hone

## Developer Certificate of Origin

hone uses the [Developer Certificate of Origin](https://developercertificate.org/) (DCO) instead of a contributor license agreement. By signing off a commit, you certify that you wrote the change or otherwise have the right to submit it under the project's licenses.

Sign off every commit:

```sh
git commit -s
```

This adds a trailer with your name and email:

```
Signed-off-by: Jane Doe <jane@example.com>
```

Commits without a sign-off cannot be merged.

## Licensing of contributions

- Code contributions are dual licensed under `MIT OR Apache-2.0`.
- Contributions to the specification and concept text (`spec/`, `docs/`) are dedicated to the public domain under CC0 1.0 Universal.

## Formatting

Markdown, JSON, and YAML files are formatted with [Prettier](https://prettier.io/), pinned in `package.json`:

```sh
npm ci
npm run format        # rewrite
npm run format:check  # check only
```

The artifact fixtures under `schemas/tests/` (`valid/`, `invalid/`, `profile/`) are excluded, because the tests check their exact bytes.

## Changing the specification

Changes to the protocol go through a public RFC: open a pull request that changes the specification and explains the motivation, the alternatives, and the impact on existing implementations. Conformance is shown through the test suite, not through the name.

## Language

All files, code, comments, documentation, and commit messages are written in English.

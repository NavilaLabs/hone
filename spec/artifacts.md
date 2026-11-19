# Artifacts

Level: Core. Schemas: `schemas/artifact.schema.json`, `schemas/common.schema.json`, the kinds of the resolved configuration. Test cases: `schemas/tests/{valid,invalid,profile,hash}/` and the gate test cases of `schema-valid`, `ids-unique`, and `links-resolve` ([conformance](conformance.md)). The gates and their codes: [gates](gates.md).

## Types, kinds, and lifecycles

1. Every artifact is a file. The core knows exactly one fixed type, `ticket`, the ticket record. Every other type is a kind defined by the configuration (`kinds`). `spec`, `decision`, `test-plan`, and `module` are presets of the package `hone/standard`, not protocol types.
2. Every kind has a lifecycle (`kinds.<type>.lifecycle`), `decided` or `living`.
3. An artifact with lifecycle `decided` belongs to a ticket, has a declared status and a content hash, and can be approved. `id`, `type`, `ticket`, `status`, `created`, and `authors` are REQUIRED. It MAY carry a `review` block ([review](review.md)).
4. An artifact with lifecycle `living` has no `ticket`, no `status`, no `review` block, and no content hash.
5. An artifact of any kind MAY have `anchors`, a list of `{ kind, ref }` with `kind` one of `path`, `glob`, and `symbol`. A `symbol` anchor has a reference `path#symbol`; a `path` or `glob` anchor has no `#`.

## IDs

6. An artifact ID is the `id_prefix` of its kind, a hyphen, and 8 characters of lowercase Crockford Base32 (`0-9`, `a-z` without `i`, `l`, `o`, `u`). An ID never changes.
7. An `id_prefix` is a lowercase letter followed by 1 to 7 lowercase letters or digits, and is unique across the kinds of a configuration. The prefixes `tk`, `ws`, `prj`, `dev`, `ev`, and `th` are reserved for the ticket record, scope IDs, events, and threads.
8. The ID of a ticket record is `tk-` and 8 characters of the same alphabet.
9. Links use only IDs. A descriptive slug lives only in the file name and MAY change.

## Data model and YAML profile

10. The data model of an artifact is JSON. Hash and gates operate on the data model, never on the bytes of the file.
11. A file is written in the strict YAML profile:
    1. YAML 1.2, UTF-8 without BOM, one document per file.
    2. Values are strings, integers in ±(2^53−1), `true`, `false`, and `null`, plus lists and maps with string keys. Floating-point numbers are not allowed.
    3. Anchors, aliases, tags, directives other than `%YAML 1.2`, merge keys (`<<`), duplicate keys, keys that are not strings, and empty values are not allowed. `null` is written as `null`.
    4. A string that a YAML parser could read as something else MUST be quoted: `yes`, `no`, `on`, `off`, `y`, `n`, `True`, `~`, `Null`, and anything that could be read as a number (`1.5`, `010`, `0x1F`, `1:30`, `9e10`). Every other string MAY stand without quotes.
    5. A date is a string `YYYY-MM-DD`, never a date type.
12. A file that violates the profile or the file shape is an error of `schema-valid` (`yaml-profile`) and is invisible to every other gate. An implementation MUST detect anchors, tags, and duplicate keys even where its YAML parser accepts them silently.

## File shapes

13. A `.md` file starts with the line `---`, followed by the front matter and a line `---`. The front matter is a map. Everything after the closing line is the body (Markdown).
14. A `.yaml` file is the data model itself, a map, and has no body.

## Files and paths

15. A file is an artifact when its path, relative to the project root, matches `ticket.path` or the `path` of a kind. A file that matches no pattern is not an artifact.
16. In a pattern, `{id}` matches the prefix of that kind, a hyphen, and the 8 characters of the random part; `{ticket}` matches a ticket ID; `{slug}` matches `[a-z0-9]+(-[a-z0-9]+)*`. Every other character matches itself.
17. Path and content MUST agree: `{id}` with `id`, `{ticket}` with `ticket` (with `id` in the ticket record), and the kind of the pattern with `type`. A disagreement is an error of `schema-valid` (`path-mismatch`). If `type` disagrees, the content is not checked further.
18. A file below the leading directories of a pattern (the part before its first placeholder) that matches no pattern but whose name starts with the prefix of a kind and a hyphen is a warning of `schema-valid` (`unmatched-file`). Every other file is ignored.

## Schema

19. `schema-valid` checks an artifact against the schema of its lifecycle (or of the ticket record), the additional schema of its kind (`kinds.<type>.schema`), the prefix of its kind, and the statuses of its kind. The profile is checked first.
20. A `type` that names neither `ticket` nor a kind of the configuration is an error (`kind-unknown`).
21. The declared status MUST be one of `kinds.<type>.statuses`. `superseded` is computed and reserved and MUST NOT appear in a file.
22. Unknown fields MUST be preserved. `schema-valid` reports an unknown field without `x-` as a warning (`unknown-field`), not as an error. Unknown means: at the top level, a key that neither the schema of the lifecycle (or of the ticket record) nor `properties` or `patternProperties` at the top of the kind schema names; in `links`, a key that is not a link type of the core. Deeper levels are not checked for unknown fields.

## Content hash

23. Only an artifact with lifecycle `decided` has a content hash. The ticket record and artifacts with lifecycle `living` have none.
24. The content hash is `sha256:` followed by the lowercase hex SHA-256 of the canonical form (RFC 8785) of the object `{ "meta": <front matter without review>, "body": <body> }`.
25. Before hashing, the body is normalized in this order:
    1. Unicode normalization form NFC.
    2. Line endings become LF.
    3. Spaces and tabs at the end of a line are removed.
    4. Leading and trailing line breaks are removed.
26. A file without a body has `body: ""`.
27. Notation of the YAML (order, quoting, style), CRLF, trailing whitespace, and the `review` block do not change the hash. Every other change to fields or body text changes it, including a new line break inside the body.

## Links

28. The link types of the core, in the map `links`:

| Link         | Target                                                                 |
| ------------ | ---------------------------------------------------------------------- |
| `supersedes` | IDs of artifacts this one replaces                                     |
| `affects`    | IDs of anchored artifacts that this artifact deliberately binds        |
| `code`       | References `path#symbol`, with several repositories `repo:path#symbol` |
| `verifies`   | IDs of the artifacts whose criteria are checked                        |
| `depends-on` | IDs                                                                    |
| `relates`    | IDs                                                                    |
| `transcript` | URLs of agent transcripts. A transcript is only linked, never stored   |

29. In a `code` reference, the path is relative, does not start with `/`, and contains no `:`. `repo` names a repository; which location belongs to the name is up to the tool.
30. The ticket of an artifact is the field `ticket`, not a link. Custom link types are named `x-<name>`.
31. Only forward links are stored. Backward links, such as "superseded by", are computed.

## Ticket record

32. The ticket record has `id`, `type: ticket`, `project`, `profile`, and `config` (the content hash of the frozen configuration), all REQUIRED, and OPTIONAL `external`, `attributes`, `overrides`, `context`, and `completed`. It is mutable and has neither status nor content hash.
33. Every entry of `context.observed` has either `module` (an ID) or `anchor` (an anchor), not both, plus `source` and `confirmed`.
34. The state of the steps lives in the process store as `step.changed` ([process store](process-store.md)). On completion, the tool records the result in `completed`: `steps_done` (REQUIRED), a list of steps, and `steps_skipped` (OPTIONAL), a list of `{ step, reason? }`.

## Status

35. Only artifacts with lifecycle `decided` have a status. The declared status is the one in the file. The effective status is computed from the declared status and the links in the checked directory.
36. A status of a kind has one property known to the protocol, `binding`. Every kind with lifecycle `decided` has at least one binding status.
37. An artifact whose declared status is binding is effectively `superseded` if an artifact in the checked directory lists it in `supersedes` and has a binding declared status. Otherwise its effective status is its declared status.
38. A superseding artifact counts by its declared status, whatever its kind and whatever its own effective status. When B supersedes A and C supersedes B, all three with a binding declared status, A and B are superseded and C keeps its status.
39. Whether a status is binding comes from the configuration the gate uses. A status that this configuration does not know is not binding.
40. The caller determines the checked directory. The protocol knows no lines, branches, or integration.
41. A change to an artifact with a binding status is a new artifact that supersedes it. The superseded file is not touched, so that its content hash and its approvals stay valid.

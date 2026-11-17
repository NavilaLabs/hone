#!/usr/bin/env python3
"""Reference check for the artifact format and the resolved configuration (protocol 1).

A helper for the specification, not an implementation of the protocol. It shows that the
YAML profile, the schemas, the content hash, and the references of the resolved
configuration from the concept (docs/protocol-concept.md, Sections 5 and 8) work together.
Dependencies: PyYAML, jsonschema, referencing.

Usage: python3 schemas/tests/run.py
Kinds come from two layers, as with `resolve`:
  schemas/standard/kinds/  presets of the package hone/standard
  schemas/tests/kinds/     custom kinds of a project layer
Directories under schemas/tests/:
  valid/           profile and schema must pass
  invalid/         profile passes, the schema must fail
  profile/invalid/ the YAML profile must fail
  config/valid/        resolved configurations: schema and references must pass
  config/invalid/      the configuration schema must fail
  config/invalid-refs/ the schema passes, the reference check must fail
"""
import hashlib
import json
import re
import sys
import unicodedata
from pathlib import Path

import yaml
from jsonschema import Draft202012Validator
from referencing import Registry, Resource
from yaml.events import (AliasEvent, DocumentStartEvent, MappingEndEvent,
                         MappingStartEvent, ScalarEvent, SequenceEndEvent,
                         SequenceStartEvent)

ROOT = Path(__file__).resolve().parent.parent
TESTS = ROOT / "tests"
MAX_INT = 2**53 - 1
ARTIFACT_ID = "https://hone.invalid/schema/1/artifact.schema.json"
CONFIG_ID = "https://hone.invalid/schema/1/config.schema.json"
CONCEPT = ROOT.parent / "docs" / "protocol-concept.md"
LIFECYCLE_DEFS = {"decided": "#/$defs/Decided", "living": "#/$defs/Living"}


class ProfileError(Exception):
    pass


BOOL_AMBIGUOUS = re.compile(r"^(?:y|Y|yes|Yes|YES|n|N|no|No|NO|on|On|ON|off|Off|OFF|True|TRUE|False|FALSE)$")
NULL_AMBIGUOUS = re.compile(r"^(?:~|Null|NULL)$")
NUMBER_LIKE = re.compile(
    r"""^[-+]?(?:\.[0-9]+|[0-9][0-9_]*(?:\.[0-9_]*)?)(?:[eE][-+]?[0-9]+)?$
      | ^0[xob][0-9a-fA-F_]+$
      | ^[-+]?\.(?:inf|Inf|INF)$
      | ^\.(?:nan|NaN|NAN)$
      | ^[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+(?:\.[0-9_]*)?$""",
    re.X,
)
PLAIN_INT = re.compile(r"^-?(?:0|[1-9][0-9]*)$")


def check_scalar(value, style, is_key):
    """Checks a scalar against the profile. Returns an error message or None."""
    if style is not None:  # quoted or block style: always a string
        return None
    if value == "":
        return "empty value (null is written as `null`)"
    if is_key and value == "<<":
        return "merge key `<<`"
    literal = value in ("true", "false", "null") or PLAIN_INT.match(value)
    if literal:
        if is_key:
            return f"key `{value}` is not a string (quote it)"
        if PLAIN_INT.match(value) and abs(int(value)) > MAX_INT:
            return f"integer `{value}` outside ±(2^53-1)"
        return None
    if BOOL_AMBIGUOUS.match(value) or NULL_AMBIGUOUS.match(value) or NUMBER_LIKE.match(value):
        return f"ambiguous value `{value}` (quote it)"
    return None


def lint(text):
    """Structural and type rules of the profile. Returns a list of errors."""
    errs, stack, docs = [], [], 0

    def done():
        if stack and stack[-1]["kind"] == "map":
            stack[-1]["expect_key"] = not stack[-1]["expect_key"]

    try:
        events = list(yaml.parse(text, Loader=yaml.SafeLoader))
    except yaml.YAMLError as e:
        return [f"YAML syntax error: {str(e).splitlines()[0]}"]

    for ev in events:
        if isinstance(ev, DocumentStartEvent):
            docs += 1
            if docs > 1:
                errs.append("several YAML documents")
            if ev.tags:
                errs.append("%TAG directive")
            if ev.version and tuple(ev.version) != (1, 2):
                errs.append(f"%YAML directive {ev.version}")
        elif isinstance(ev, AliasEvent):
            errs.append(f"alias *{ev.anchor}")
            done()
        elif isinstance(ev, (ScalarEvent, SequenceStartEvent, MappingStartEvent)):
            if ev.anchor:
                errs.append(f"anchor &{ev.anchor}")
            if ev.tag:
                errs.append(f"tag {ev.tag}")
            top = stack[-1] if stack else None
            is_key = bool(top and top["kind"] == "map" and top["expect_key"])
            if isinstance(ev, ScalarEvent):
                msg = check_scalar(ev.value, ev.style, is_key)
                if msg:
                    errs.append(msg)
                if is_key:
                    if ev.value in top["keys"]:
                        errs.append(f"duplicate key `{ev.value}`")
                    top["keys"].add(ev.value)
                done()
            else:
                if is_key:
                    errs.append("key must be a scalar")
                stack.append({"kind": "map" if isinstance(ev, MappingStartEvent) else "seq",
                              "keys": set(), "expect_key": True})
        elif isinstance(ev, (MappingEndEvent, SequenceEndEvent)):
            stack.pop()
            done()
    return errs


class ProfileLoader(yaml.SafeLoader):
    """Loads only the JSON types of the profile: true/false, null, integers, strings."""


ProfileLoader.yaml_implicit_resolvers = {}
ProfileLoader.add_implicit_resolver("tag:yaml.org,2002:bool", re.compile(r"^(?:true|false)$"), list("tf"))
ProfileLoader.add_implicit_resolver("tag:yaml.org,2002:null", re.compile(r"^null$"), ["n"])
ProfileLoader.add_implicit_resolver("tag:yaml.org,2002:int", re.compile(r"^-?(?:0|[1-9][0-9]*)$"), list("-0123456789"))


def parse_artifact(path):
    """Reads an artifact file. Returns (data model, body)."""
    raw = Path(path).read_bytes()
    if raw.startswith(b"\xef\xbb\xbf"):
        raise ProfileError("BOM not allowed")
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as e:
        raise ProfileError(f"not valid UTF-8: {e}")
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    if Path(path).suffix == ".md":
        lines = text.split("\n")
        if lines[0] != "---":
            raise ProfileError("front matter missing: the first line must be ---")
        try:
            end = lines.index("---", 1)
        except ValueError:
            raise ProfileError("front matter not closed")
        front, body = "\n".join(lines[1:end]), "\n".join(lines[end + 1:])
    else:
        front, body = text, ""
    errs = lint(front)
    if errs:
        raise ProfileError("; ".join(errs))
    meta = yaml.load(front, Loader=ProfileLoader)
    if not isinstance(meta, dict):
        raise ProfileError("front matter must be a map")
    return meta, body


TRAILING_BLANKS = re.compile(r"[ \t]+$", re.M)


def normalize_body(body):
    text = unicodedata.normalize("NFC", body)
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    return TRAILING_BLANKS.sub("", text).strip("\n")


def jcs(v):
    """RFC 8785 for the subset of the profile: null, bool, integer, string, list, map."""
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError(f"not in the data model: {type(v)}")


def content_hash(meta, body):
    doc = {"meta": {k: v for k, v in meta.items() if k != "review"}, "body": normalize_body(body)}
    return "sha256:" + hashlib.sha256(jcs(doc).encode("utf-8")).hexdigest()


def load_kinds():
    """Kinds from the standard package and the project layer, the higher layer wins.

    Returns {name: definition} as in the resolved configuration: a relative `schema` path
    is replaced by the embedded schema.
    """
    kinds = {}
    for layer in (ROOT / "standard", TESTS):
        for path in sorted((layer / "kinds").glob("*.yaml")):
            definition, _ = parse_artifact(path)
            if "schema" in definition:
                definition["schema"] = json.loads((layer / definition["schema"]).read_text(encoding="utf-8"))
            kinds[path.stem] = definition
    return kinds


def make_registry():
    registry = Registry()
    for path in (ROOT / "common.schema.json", ROOT / "artifact.schema.json", ROOT / "config.schema.json"):
        schema = json.loads(path.read_text(encoding="utf-8"))
        Draft202012Validator.check_schema(schema)
        registry = registry.with_resource(schema["$id"], Resource.from_contents(schema))
    return registry


class SchemaValid:
    """The gate schema-valid for a resolved set of kinds."""

    def __init__(self, kinds):
        self.kinds = kinds
        self.registry = make_registry()
        self.validators = {}

    def _validator(self, kind_name):
        if kind_name not in self.validators:
            if kind_name == "ticket":
                parts = [{"$ref": ARTIFACT_ID}]
            else:
                kind = self.kinds[kind_name]
                parts = [{"$ref": ARTIFACT_ID}, {"$ref": ARTIFACT_ID + LIFECYCLE_DEFS[kind["lifecycle"]]}]
                if "schema" in kind:
                    parts.append(kind["schema"])
            schema = {"$schema": "https://json-schema.org/draft/2020-12/schema", "allOf": parts}
            self.validators[kind_name] = Draft202012Validator(
                schema, registry=self.registry, format_checker=Draft202012Validator.FORMAT_CHECKER)
        return self.validators[kind_name]

    def first_error(self, meta):
        kind_name = meta.get("type")
        if kind_name != "ticket":
            if kind_name not in self.kinds:
                return f"type: unknown kind `{kind_name}`"
            prefix = self.kinds[kind_name]["id_prefix"] + "-"
            if not str(meta.get("id", "")).startswith(prefix):
                return f"id: must start with `{prefix}` (id_prefix of the kind)"
            statuses = self.kinds[kind_name].get("statuses", {})
            if "status" in meta and statuses and meta["status"] not in statuses:
                return f"status: `{meta['status']}` is not a status of the kind `{kind_name}`"
        errs = sorted(self._validator(kind_name).iter_errors(meta), key=lambda e: list(e.absolute_path))
        if not errs:
            return None
        e = errs[0]
        where = "/".join(str(p) for p in e.absolute_path) or "(root)"
        return f"{where}: {e.message[:110]}"


STANDARD_GATES = {"artifact-exists", "status-is", "schema-valid", "ids-unique", "links-resolve",
                  "no-open-blocking-threads", "threads-answered", "approvals-valid", "steps-complete",
                  "change-needs-binding"}


def reference_errors(config):
    """The references of a resolved configuration that JSON Schema cannot check."""
    errs = []
    kinds, roles, steps = config["kinds"], config["roles"], config["steps"]
    plugins = config.get("plugins", {})

    def need_role(where, role, capability=None):
        if role not in roles:
            errs.append(f"{where}: unknown role `{role}`")
        elif capability and capability not in roles[role]["can"]:
            errs.append(f"{where}: role `{role}` cannot `{capability}`")

    def need_kind(where, kind, allow_ticket=False, decided=False):
        if allow_ticket and kind == "ticket":
            return
        if kind not in kinds:
            errs.append(f"{where}: unknown kind `{kind}`")
        elif decided and kinds[kind]["lifecycle"] != "decided":
            errs.append(f"{where}: kind `{kind}` must have lifecycle `decided`")

    def need_statuses(where, kind, selector):
        if kind not in kinds:
            return
        if kinds[kind]["lifecycle"] != "decided":
            errs.append(f"{where}: kind `{kind}` has no statuses (lifecycle `living`)")
            return
        for status in selector if isinstance(selector, list) else [selector]:
            if status != "superseded" and status not in kinds[kind]["statuses"]:
                errs.append(f"{where}: `{status}` is not a status of the kind `{kind}`")

    def check_gate(where, entry):
        name, params = entry["gate"], entry.get("params", {})
        if name == "exec":
            if "evaluate" in params and params["evaluate"] not in plugins:
                errs.append(f"{where}/evaluate: unknown plugin `{params['evaluate']}`")
        elif name in ("artifact-exists", "status-is"):
            need_kind(f"{where}/type", params["type"])
            if "status" in params:
                need_statuses(f"{where}/status", params["type"], params["status"])
        elif name == "threads-answered":
            need_role(f"{where}/role", params["role"])
        elif name == "approvals-valid":
            need_role(f"{where}/role", params["role"], "approve")
            if "type" in params:
                need_kind(f"{where}/type", params["type"], decided=True)
        elif name == "change-needs-binding":
            need_kind(f"{where}/anchored", params["anchored"])
            need_kind(f"{where}/binding", params["binding"], decided=True)
        elif name not in STANDARD_GATES and name not in plugins:
            errs.append(f"{where}: unknown gate `{name}`")

    prefixes = {}
    for name, kind in kinds.items():
        other = prefixes.setdefault(kind["id_prefix"], name)
        if other != name:
            errs.append(f"kinds/{name}/id_prefix: `{kind['id_prefix']}` is already used by `{other}`")
    for handle, assigned in config["actors"].items():
        for role in assigned:
            need_role(f"actors/{handle}", role)
    for name, step in steps.items():
        where = f"steps/{name}"
        for role in step["acts"]:
            need_role(f"{where}/acts", role)
        if "decides" in step:
            need_role(f"{where}/decides", step["decides"])
        for field in ("inputs", "outputs"):
            for kind in step[field]:
                need_kind(f"{where}/{field}", kind, allow_ticket=True)
        for required in step["requires"]:
            if required not in steps:
                errs.append(f"{where}/requires: unknown step `{required}`")
        for label, entry in step["gates"].items():
            check_gate(f"{where}/gates/{label}", entry)

    state = {}

    def visit(name, path):
        state[name] = "open"
        for required in steps[name]["requires"]:
            if state.get(required) == "open":
                errs.append("steps: cycle in requires: " + " -> ".join(path + [required]))
            elif required in steps and required not in state:
                visit(required, path + [required])
        state[name] = "done"

    for name in steps:
        if name not in state:
            visit(name, [name])

    for name, profile in config["profiles"].items():
        for step in profile["steps"]:
            if step not in steps:
                errs.append(f"profiles/{name}/steps: unknown step `{step}`")
    for i, rule in enumerate(config["select"]["rules"]):
        if rule["profile"] not in config["profiles"]:
            errs.append(f"select/rules/{i}/profile: unknown profile `{rule['profile']}`")
    if config["select"]["default"] not in config["profiles"]:
        errs.append(f"select/default: unknown profile `{config['select']['default']}`")
    return errs


def concept_example():
    """The Level 1 example of the concept, parsed with the YAML profile."""
    text = CONCEPT.read_text(encoding="utf-8")
    section = text[text.index("### Level 1: the resolved configuration"):]
    start = section.index("```yaml\n") + len("```yaml\n")
    block = section[start:section.index("```\n", start)]
    errs = lint(block)
    if errs:
        raise ProfileError("; ".join(errs))
    return yaml.load(block, Loader=ProfileLoader)


def main():
    failed = 0

    def report(ok, label, detail=""):
        nonlocal failed
        failed += 0 if ok else 1
        print(f"  {'ok  ' if ok else 'FAIL'} {label}" + (f"  -> {detail}" if detail else ""))

    kinds = load_kinds()
    validator = SchemaValid(kinds)
    registry = validator.registry
    config_schema = registry.contents(CONFIG_ID)
    config_validator = Draft202012Validator(config_schema, registry=registry)
    kind_validator = Draft202012Validator({"$ref": CONFIG_ID + "#/$defs/Kind"}, registry=registry)

    def first_config_error(v, data):
        errs = sorted(v.iter_errors(data), key=lambda e: list(e.absolute_path))
        if not errs:
            return None
        e = errs[0]
        return f"{'/'.join(str(p) for p in e.absolute_path) or '(root)'}: {e.message[:110]}"

    print("kinds/")
    for name, definition in kinds.items():
        err = first_config_error(kind_validator, definition)
        report(err is None, name, err or "")

    print("valid/")
    for p in sorted((TESTS / "valid").iterdir()):
        try:
            meta, _ = parse_artifact(p)
            err = validator.first_error(meta)
            report(err is None, p.name, err or "")
        except ProfileError as e:
            report(False, p.name, f"profile: {e}")

    print("invalid/")
    for p in sorted((TESTS / "invalid").iterdir()):
        try:
            meta, _ = parse_artifact(p)
        except ProfileError as e:
            report(False, p.name, f"already fails the profile: {e}")
            continue
        err = validator.first_error(meta)
        report(err is not None, p.name, err or "was wrongly accepted")

    print("profile/invalid/")
    for p in sorted((TESTS / "profile" / "invalid").iterdir()):
        try:
            parse_artifact(p)
            report(False, p.name, "was wrongly accepted")
        except ProfileError as e:
            report(True, p.name, str(e))

    print("hash")
    src = TESTS / "valid" / "decision-accepted.md"
    meta, body = parse_artifact(src)
    h = content_hash(meta, body)
    stored = meta["review"]["approved_by"][0]["hash"]
    report(stored == h, "approval in the review block matches the content", f"{h[:19]}…")

    reformatted = "---\n" + yaml.safe_dump(meta, sort_keys=True, allow_unicode=True, default_flow_style=False) + "---\n" + body
    tmp = TESTS / ".reformatted.md"
    try:
        tmp.write_text(reformatted, encoding="utf-8")
        m2, b2 = parse_artifact(tmp)
        report(content_hash(m2, b2) == h, "reformatted YAML (order, quoting, style) does not change the hash")
        tmp.write_bytes(src.read_bytes().replace(b"\n", b"\r\n"))
        m3, b3 = parse_artifact(tmp)
        report(content_hash(m3, b3) == h, "CRLF instead of LF does not change the hash")
    finally:
        tmp.unlink(missing_ok=True)

    changed = dict(meta, review={"approved_by": []})
    report(content_hash(changed, body) == h, "a change to the review block does not change the hash")
    report(content_hash(dict(meta, status="rejected"), body) != h, "a change to the status changes the hash")
    report(content_hash(meta, body + "\nOne more sentence.") != h, "a change to the body changes the hash")
    trailing = body.replace("\n", "  \t\n")
    report(content_hash(meta, trailing) == h, "trailing spaces and tabs do not change the hash")
    decomposed = unicodedata.normalize("NFD", body)
    report(decomposed != body and content_hash(meta, decomposed) == h,
           "Unicode NFD instead of NFC does not change the hash")
    first_break = body.index(" ", body.index("Synchronous processing"))
    rewrapped = body[:first_break] + "\n" + body[first_break + 1:]
    report(content_hash(meta, rewrapped) != h, "a new line break inside a paragraph changes the hash")

    print("config/valid/")
    for p in sorted((TESTS / "config" / "valid").iterdir()):
        config = json.loads(p.read_text(encoding="utf-8"))
        err = first_config_error(config_validator, config)
        if err is None:
            refs = reference_errors(config)
            err = refs[0] if refs else None
        report(err is None, p.name, err or "")

    print("config/invalid/")
    for p in sorted((TESTS / "config" / "invalid").iterdir()):
        err = first_config_error(config_validator, json.loads(p.read_text(encoding="utf-8")))
        report(err is not None, p.name, err or "was wrongly accepted")

    print("config/invalid-refs/")
    for p in sorted((TESTS / "config" / "invalid-refs").iterdir()):
        config = json.loads(p.read_text(encoding="utf-8"))
        err = first_config_error(config_validator, config)
        if err is not None:
            report(False, p.name, f"already fails the schema: {err}")
            continue
        refs = reference_errors(config)
        report(bool(refs), p.name, refs[0] if refs else "was wrongly accepted")

    print("concept")
    fixture = json.loads((TESTS / "config" / "valid" / "concept-example.json").read_text(encoding="utf-8"))
    try:
        report(concept_example() == fixture, "the Level 1 example in Section 8 equals config/valid/concept-example.json")
    except ProfileError as e:
        report(False, "the Level 1 example in Section 8 follows the YAML profile", str(e))

    print(f"\n{'ALL OK' if failed == 0 else str(failed) + ' FAILED'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

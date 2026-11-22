//! The declarations each language's rules find in a sample, with their line, visibility, symbol,
//! and text, and the full public API entries of a second sample. The expected values were
//! written by hand from the samples.

use serde_json::{Value, json};

fn declarations(
    rules: &lang_tree::Rules,
    extension: &str,
    source: &str,
) -> Vec<(usize, bool, String, String)> {
    lang_tree::declarations_of(rules, extension, source)
        .expect("the sample parses")
        .into_iter()
        .map(|(public, entry)| {
            (
                usize::try_from(entry["line"].as_u64().expect("a line")).expect("a small line"),
                public,
                entry["symbol"].as_str().expect("a symbol").to_owned(),
                entry["text"].as_str().expect("a text").to_owned(),
            )
        })
        .collect()
}

fn entries(rules: &lang_tree::Rules, extension: &str, source: &str) -> Vec<Value> {
    lang_tree::declarations_of(rules, extension, source)
        .expect("the sample parses")
        .into_iter()
        .filter(|(public, _)| *public)
        .map(|(_, entry)| entry)
        .collect()
}

#[test]
fn typescript_declarations() {
    let expected: Vec<(usize, bool, String, String)> = vec![
        (
            1,
            true,
            "createUser".into(),
            "function createUser(name: string): User".into(),
        ),
        (2, false, "helper".into(), "function helper()".into()),
        (3, true, "Fee".into(), "class Fee".into()),
        (3, false, "Fee.secret".into(), "private secret".into()),
        (3, false, "Fee.#hidden".into(), "#hidden".into()),
        (3, true, "Fee.amount".into(), "amount: number".into()),
        (3, true, "Fee.add".into(), "add(x: number): number".into()),
        (3, true, "Fee.of".into(), "static of(): Fee".into()),
        (4, true, "User".into(), "interface User".into()),
        (4, true, "User.name".into(), "name: string".into()),
        (4, true, "User.greet".into(), "greet(): void".into()),
        (5, true, "Id".into(), "type Id = string".into()),
        (6, true, "X".into(), "const X: number".into()),
        (6, true, "Y".into(), "const Y".into()),
        (7, true, "Color".into(), "enum Color".into()),
        (7, true, "Color.Red".into(), "Red".into()),
        (7, true, "Color.Green".into(), "Green".into()),
        (8, true, "Payments".into(), "namespace Payments".into()),
        (8, true, "Payments.Inner".into(), "class Inner".into()),
        (8, false, "Payments.Hidden".into(), "class Hidden".into()),
        (9, true, "Base".into(), "abstract class Base".into()),
        (9, true, "Base.run".into(), "abstract run(): void".into()),
    ];
    assert_eq!(
        declarations(
            &lang_tree::typescript::RULES,
            "ts",
            "export function createUser(name: string): User { return x }\nfunction helper() {}\nexport class Fee { private secret = 1; #hidden = 2; amount: number = 1; add(x: number): number { return x } static of(): Fee { return new Fee() } }\nexport interface User { name: string; greet(): void }\nexport type Id = string;\nexport const X: number = 1, Y = 2;\nexport enum Color { Red, Green = 2 }\nexport namespace Payments { export class Inner {} class Hidden {} }\nexport abstract class Base { abstract run(): void }\n"
        ),
        expected
    );
}

#[test]
fn python_declarations() {
    let expected: Vec<(usize, bool, String, String)> = vec![
        (
            1,
            true,
            "create_user".into(),
            "def create_user(name: str) -> User".into(),
        ),
        (4, true, "Fee".into(), "class Fee(Base)".into()),
        (5, true, "Fee.rate".into(), "rate: int".into()),
        (
            6,
            true,
            "Fee.add".into(),
            "def add(self, x: int) -> int".into(),
        ),
        (8, false, "Fee._private".into(), "def _private(self)".into()),
        (9, true, "Fee.__init__".into(), "def __init__(self)".into()),
        (11, true, "Money".into(), "class Money".into()),
        (12, true, "Money.amount".into(), "amount: int".into()),
        (13, true, "LIMIT".into(), "LIMIT: int".into()),
        (14, false, "_hidden".into(), "_hidden".into()),
    ];
    assert_eq!(
        declarations(
            &lang_tree::python::RULES,
            "py",
            "def create_user(name: str) -> User:\n    x = 1\n    return 1\nclass Fee(Base):\n    rate: int = 1\n    def add(self, x: int) -> int:\n        return x\n    def _private(self): pass\n    def __init__(self): pass\n@dataclass\nclass Money:\n    amount: int\nLIMIT: int = 5\n_hidden = 1\n"
        ),
        expected
    );
}

#[test]
fn php_declarations() {
    let expected: Vec<(usize, bool, String, String)> = vec![
        (3, true, "fee".into(), "function fee(int $x): int".into()),
        (4, true, "LIMIT".into(), "const LIMIT".into()),
        (
            5,
            true,
            "Fee".into(),
            "class Fee extends Base implements I".into(),
        ),
        (5, true, "Fee::RATE".into(), "public const RATE".into()),
        (
            5,
            false,
            "Fee::$secret".into(),
            "private int $secret".into(),
        ),
        (5, true, "Fee::$amount".into(), "public ?int $amount".into()),
        (
            5,
            true,
            "Fee::add".into(),
            "public function add(int $x): int".into(),
        ),
        (
            5,
            false,
            "Fee::hidden".into(),
            "protected function hidden()".into(),
        ),
        (
            5,
            true,
            "Fee::of".into(),
            "public static function of(): self".into(),
        ),
        (6, true, "I".into(), "interface I".into()),
        (
            6,
            true,
            "I::add".into(),
            "public function add(int $x): int".into(),
        ),
        (7, true, "Color".into(), "enum Color: string".into()),
        (7, true, "Color::Red".into(), "case Red = 'r'".into()),
    ];
    assert_eq!(
        declarations(
            &lang_tree::php::RULES,
            "php",
            "<?php\nnamespace App\\Pay;\nfunction fee(int $x): int { return $x; }\nconst LIMIT = 5;\nclass Fee extends Base implements I { public const RATE = 1; private int $secret; public ?int $amount = null; public function add(int $x): int { return $x; } protected function hidden() {} public static function of(): self {} }\ninterface I { public function add(int $x): int; }\nenum Color: string { case Red = 'r'; }\n"
        ),
        expected
    );
}

#[test]
fn dart_declarations() {
    let expected: Vec<(usize, bool, String, String)> = vec![
        (1, true, "fee".into(), "int fee(int x)".into()),
        (2, true, "limit".into(), "final limit".into()),
        (3, false, "_hidden".into(), "const _hidden".into()),
        (4, true, "Fee".into(), "class Fee extends Base".into()),
        (4, true, "Fee.amount".into(), "int amount".into()),
        (4, false, "Fee._secret".into(), "int _secret".into()),
        (4, true, "Fee.add".into(), "int add(int x)".into()),
        (4, true, "Fee.of".into(), "static Fee of()".into()),
        (4, true, "Fee.Fee".into(), "Fee(this.amount)".into()),
        (4, true, "Fee.value".into(), "int get value".into()),
        (5, true, "M".into(), "mixin M".into()),
        (5, true, "M.m".into(), "void m()".into()),
        (6, true, "Color".into(), "enum Color".into()),
        (6, true, "Color.red".into(), "red".into()),
        (6, true, "Color.green".into(), "green".into()),
        (7, true, "E".into(), "extension E on Fee".into()),
        (7, true, "E.twice".into(), "int twice()".into()),
        (8, true, "Shape".into(), "abstract class Shape".into()),
        (8, true, "Shape.area".into(), "double area()".into()),
    ];
    assert_eq!(
        declarations(
            &lang_tree::dart::RULES,
            "dart",
            "int fee(int x) => x;\nfinal limit = 5;\nconst _hidden = 1;\nclass Fee extends Base { int amount = 1; int _secret = 2; int add(int x) { return x; } static Fee of() => Fee(); Fee(this.amount); int get value => amount; }\nmixin M { void m() {} }\nenum Color { red, green }\nextension E on Fee { int twice() => 2; }\nabstract class Shape { double area(); }\n"
        ),
        expected
    );
}

#[test]
fn a_syntax_error_names_its_line() {
    let error = lang_tree::declarations_of(
        &lang_tree::python::RULES,
        "py",
        "def ok():\n    pass\ndef broken(:\n",
    )
    .expect_err("not Python");
    assert_eq!(error, "syntax error at line 3");
}

#[test]
fn typescript_entries() {
    let source = "@sealed\nexport abstract class Fee<T extends Base = Base> extends Base<T> implements Priced, Other {\n  static of(x: number, y?: string, z = 3, ...rest: number[]): Fee<T> { return 1 }\n  readonly amount: number = 4;\n  async get total(): Promise<number> { return 1 }\n}\nexport function f<K>(a: Map<\n  K,\n  V,\n>): void {}\nexport type U =\n  | A\n  | B;\nexport enum E { A = 1 }\nexport interface I extends J { p?: string }\n";
    assert_eq!(
        entries(&lang_tree::typescript::RULES, "ts", source),
        vec![
            json!({ "symbol": "Fee", "kind": "class", "type_parameters": ["T extends Base=Base"], "bases": ["Base<T>", "Priced", "Other"], "modifiers": ["abstract"], "attributes": ["@sealed"], "line": 2, "text": "abstract class Fee<T extends Base = Base> extends Base<T> implements Priced, Other" }),
            json!({ "symbol": "Fee.of", "kind": "method", "static": true, "parameters": [
                { "name": "x", "type": "number", "passing": "positional" },
                { "name": "y", "type": "string", "optional": true, "passing": "positional" },
                { "name": "z", "optional": true, "default": "3", "passing": "positional" },
                { "name": "rest", "type": "number[]", "variadic": true, "passing": "positional" }
            ], "returns": "Fee<T>", "line": 3, "text": "static of(x: number, y?: string, z = 3, ...rest: number[]): Fee<T>" }),
            json!({ "symbol": "Fee.amount", "kind": "field", "modifiers": ["readonly"], "type": "number", "line": 4, "text": "readonly amount: number" }),
            json!({ "symbol": "Fee.total", "kind": "getter", "modifiers": ["async"], "returns": "Promise<number>", "line": 5, "text": "async get total(): Promise<number>" }),
            json!({ "symbol": "f", "kind": "function", "type_parameters": ["K"], "parameters": [{ "name": "a", "type": "Map<K, V>", "passing": "positional" }], "returns": "void", "line": 7, "text": "function f<K>(a: Map< K, V, >): void" }),
            json!({ "symbol": "U", "kind": "type", "type": "A|B", "line": 11, "text": "type U = | A | B" }),
            json!({ "symbol": "E", "kind": "enum", "line": 14, "text": "enum E" }),
            json!({ "symbol": "E.A", "kind": "variant", "value": "1", "line": 14, "text": "A" }),
            json!({ "symbol": "I", "kind": "interface", "bases": ["J"], "line": 15, "text": "interface I extends J" }),
            json!({ "symbol": "I.p", "kind": "field", "modifiers": ["?"], "type": "string", "line": 15, "text": "p?: string" }),
        ]
    );
}

#[test]
fn python_entries() {
    let source = "@dataclass\nclass Fee(Base, metaclass=M):\n    async def add(self, a, /, b: int = 2, *args, c, d: str = \"x\", **kw) -> int:\n        pass\n    @staticmethod\n    def of(x: int, *, y=1): pass\n    @classmethod\n    def make(cls, z): pass\n    @property\n    def total(self) -> int: pass\ndef top(self): pass\n";
    assert_eq!(
        entries(&lang_tree::python::RULES, "py", source),
        vec![
            json!({ "symbol": "Fee", "kind": "class", "bases": ["Base", "metaclass=M"], "attributes": ["@dataclass"], "line": 2, "text": "class Fee(Base, metaclass=M)" }),
            json!({ "symbol": "Fee.add", "kind": "method", "modifiers": ["async"], "parameters": [
                { "name": "a", "passing": "positional" },
                { "name": "b", "type": "int", "optional": true, "default": "2", "passing": "both" },
                { "name": "args", "variadic": true, "passing": "positional" },
                { "name": "c", "passing": "named" },
                { "name": "d", "type": "str", "optional": true, "default": "\"x\"", "passing": "named" },
                { "name": "kw", "variadic": true, "passing": "named" }
            ], "returns": "int", "line": 3, "text": "async def add(self, a, /, b: int = 2, *args, c, d: str = \"x\", **kw) -> int" }),
            json!({ "symbol": "Fee.of", "kind": "method", "static": true, "parameters": [
                { "name": "x", "type": "int", "passing": "both" },
                { "name": "y", "optional": true, "default": "1", "passing": "named" }
            ], "line": 6, "text": "def of(x: int, *, y=1)" }),
            json!({ "symbol": "Fee.make", "kind": "method", "static": true, "parameters": [{ "name": "z", "passing": "both" }], "line": 8, "text": "def make(cls, z)" }),
            json!({ "symbol": "Fee.total", "kind": "property", "returns": "int", "line": 10, "text": "def total(self) -> int" }),
            json!({ "symbol": "top", "kind": "function", "parameters": [{ "name": "self", "passing": "both" }], "line": 11, "text": "def top(self)" }),
        ]
    );
}

#[test]
fn php_entries() {
    let source = "<?php\n#[Attr(1)]\nfinal class Fee extends Base implements I, J {\n    public const int RATE = 1;\n    public readonly ?int $amount;\n    public static function of(int $x, ?string $y = null, int ...$rest): static {}\n    public function __construct() {}\n}\nenum Color: string implements HasLabel { case Red = 'r'; }\n";
    assert_eq!(
        entries(&lang_tree::php::RULES, "php", source),
        vec![
            json!({ "symbol": "Fee", "kind": "class", "bases": ["Base", "I", "J"], "modifiers": ["final"], "attributes": ["Attr(1)"], "line": 2, "text": "#[Attr(1)] final class Fee extends Base implements I, J" }),
            json!({ "symbol": "Fee::RATE", "kind": "constant", "type": "int", "value": "1", "line": 4, "text": "public const int RATE" }),
            json!({ "symbol": "Fee::$amount", "kind": "field", "modifiers": ["readonly"], "type": "?int", "line": 5, "text": "public readonly ?int $amount" }),
            json!({ "symbol": "Fee::of", "kind": "method", "static": true, "parameters": [
                { "name": "$x", "type": "int", "passing": "both" },
                { "name": "$y", "type": "?string", "optional": true, "default": "null", "passing": "both" },
                { "name": "$rest", "type": "int", "variadic": true, "passing": "both" }
            ], "returns": "static", "line": 6, "text": "public static function of(int $x, ?string $y = null, int ...$rest): static" }),
            json!({ "symbol": "Fee::__construct", "kind": "constructor", "line": 7, "text": "public function __construct()" }),
            json!({ "symbol": "Color", "kind": "enum", "bases": ["HasLabel"], "type": "string", "line": 9, "text": "enum Color: string implements HasLabel" }),
            json!({ "symbol": "Color::Red", "kind": "variant", "value": "'r'", "line": 9, "text": "case Red = 'r'" }),
        ]
    );
}

#[test]
fn dart_entries() {
    let source = "@immutable\nabstract class Fee<T extends num> extends Base<T> with M implements I {\n  static const int rate = 1;\n  final int amount;\n  Fee(this.amount, {required int a, String? b = 'x'});\n  static Fee of(int x, [int y = 2]) => Fee(x, a: 1);\n  factory Fee.named(int x) => Fee(x, a: 1);\n}\nenum Color { red }\nextension E on Fee { int twice() => 2; }\nconst limit = 5;\n";
    assert_eq!(
        entries(&lang_tree::dart::RULES, "dart", source),
        vec![
            json!({ "symbol": "Fee", "kind": "class", "type_parameters": ["T extends num"], "bases": ["Base<T>", "M", "I"], "modifiers": ["abstract"], "attributes": ["@immutable"], "line": 1, "text": "@immutable abstract class Fee<T extends num> extends Base<T> with M implements I" }),
            json!({ "symbol": "Fee.rate", "kind": "field", "static": true, "modifiers": ["const"], "type": "int", "value": "1", "line": 3, "text": "static const int rate" }),
            json!({ "symbol": "Fee.amount", "kind": "field", "modifiers": ["final"], "type": "int", "line": 4, "text": "final int amount" }),
            json!({ "symbol": "Fee.Fee", "kind": "constructor", "parameters": [
                { "name": "amount", "passing": "positional" },
                { "name": "a", "type": "int", "passing": "named" },
                { "name": "b", "type": "String?", "optional": true, "default": "'x'", "passing": "named" }
            ], "line": 5, "text": "Fee(this.amount, {required int a, String? b = 'x'})" }),
            json!({ "symbol": "Fee.of", "kind": "method", "static": true, "parameters": [
                { "name": "x", "type": "int", "passing": "positional" },
                { "name": "y", "type": "int", "optional": true, "default": "2", "passing": "positional" }
            ], "returns": "Fee", "line": 6, "text": "static Fee of(int x, [int y = 2])" }),
            json!({ "symbol": "Fee.Fee", "kind": "constructor", "modifiers": ["factory"], "parameters": [{ "name": "x", "type": "int", "passing": "positional" }], "line": 7, "text": "factory Fee.named(int x)" }),
            json!({ "symbol": "Color", "kind": "enum", "line": 9, "text": "enum Color" }),
            json!({ "symbol": "Color.red", "kind": "variant", "line": 9, "text": "red" }),
            json!({ "symbol": "E", "kind": "extension", "self_type": "Fee", "line": 10, "text": "extension E on Fee" }),
            json!({ "symbol": "E.twice", "kind": "method", "returns": "int", "line": 10, "text": "int twice()" }),
            json!({ "symbol": "limit", "kind": "variable", "modifiers": ["const"], "value": "5", "line": 11, "text": "const limit" }),
        ]
    );
}

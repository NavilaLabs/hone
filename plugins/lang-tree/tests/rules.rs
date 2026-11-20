//! The declarations each language's rules find in a sample, with their visibility, signature,
//! and line. The expected lists were checked by hand against the samples.

fn declarations(
    rules: &lang_tree::Rules,
    extension: &str,
    source: &str,
) -> Vec<(usize, bool, String, String)> {
    lang_tree::declarations_of(rules, extension, source)
        .expect("the sample parses")
        .into_iter()
        .map(|(symbol, public, signature, line)| (line, public, symbol, signature))
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

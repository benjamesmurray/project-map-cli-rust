use fixture_crate::a::b::Sym;
use serde_json::Value;

#[test]
fn symbol_and_value() {
    let _ = Sym;
    let _ = Value::Null;
}

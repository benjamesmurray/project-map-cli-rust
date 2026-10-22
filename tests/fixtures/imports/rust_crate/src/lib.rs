extern crate serde_json;

pub mod a;
pub mod c;
pub mod errors;
pub mod user;

use serde_json::Value;

pub fn payload() -> Value {
    Value::Null
}

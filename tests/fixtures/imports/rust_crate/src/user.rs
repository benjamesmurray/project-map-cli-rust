use crate::a::b::Sym;
use crate::a::{b, c};
use std::collections::HashMap;

pub fn sym_count() -> usize {
    let mut m: HashMap<usize, Sym> = HashMap::new();
    m.insert(0, b::Sym);
    let _ = c::Delta;
    m.len()
}

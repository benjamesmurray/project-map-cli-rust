struct User {
    username: String,
}

impl User {
    fn get_name(&self) -> &str {
        &self.username
    }
}

trait Greeter {
    fn greet(&self);
}

fn main() {}

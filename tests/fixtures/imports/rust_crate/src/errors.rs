pub struct E;

impl From<std::io::Error> for E {
    fn from(_: std::io::Error) -> Self {
        E
    }
}

impl From<std::fmt::Error> for E {
    fn from(_: std::fmt::Error) -> Self {
        E
    }
}

pub enum MessageKind {
    Normal,
    Warning,
    Error,
}

impl Default for MessageKind {
    fn default() -> Self {
        MessageKind::Normal
    }
}

impl From<MessageKind> for &str {
    fn from(f: MessageKind) -> Self {
        match f {
            MessageKind::Normal => "normal",
            MessageKind::Warning => "warning",
            MessageKind::Error => "error",
        }
    }
}

impl From<MessageKind> for String {
    fn from(f: MessageKind) -> Self {
        String::from(<&str>::from(f))
    }
}

impl From<MessageKind> for u8 {
    fn from(f: MessageKind) -> Self {
        match f {
            MessageKind::Normal => 0,
            MessageKind::Warning => 1,
            MessageKind::Error => 2,
        }
    }
}

impl std::convert::TryFrom<u8> for MessageKind {
    type Error = String;
    fn try_from(n: u8) -> std::result::Result<Self, <Self as std::convert::TryFrom<u8>>::Error> {
        match n {
            0 => Ok(MessageKind::Normal),
            1 => Ok(MessageKind::Warning),
            2 => Ok(MessageKind::Error),
            _ => Err(String::from("invalid MessageKind value")),
        }
    }
}

pub trait KeyValPrint {
    fn print(&self, kind: Option<MessageKind>, key: &str, val: &str);
}

pub struct DontPrint;

impl KeyValPrint for DontPrint {
    fn print(&self, _kind: Option<MessageKind>, _key: &str, _val: &str) {}
}

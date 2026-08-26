/// Owned binary-safe Lua string bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LuaBytes(Vec<u8>);

impl LuaBytes {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl From<Vec<u8>> for LuaBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl From<LuaBytes> for Vec<u8> {
    fn from(bytes: LuaBytes) -> Self {
        bytes.0
    }
}

impl AsRef<[u8]> for LuaBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

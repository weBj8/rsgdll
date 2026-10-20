//! Lua realms (spec §6.1, §8.2).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Realm {
    Server,
    Client,
    Menu,
}

impl Realm {
    pub fn as_str(self) -> &'static str {
        match self {
            Realm::Server => "server",
            Realm::Client => "client",
            Realm::Menu => "menu",
        }
    }

    /// `ILuaShared::GetLuaInterface` id.
    pub fn id(self) -> u8 {
        use rsgdll_sys::gmod::realm::*;
        match self {
            Realm::Server => SERVER,
            Realm::Client => CLIENT,
            Realm::Menu => MENU,
        }
    }
}

impl crate::Lua {
    /// Realm of this state (from `ILuaInterface`, or the crate's first realm outside GMod).
    pub fn realm(&self) -> Option<Realm> {
        *self.runtime().ext::<Option<Realm>>("realm", || None)
    }

    pub fn is_server(&self) -> bool {
        self.realm() == Some(Realm::Server)
    }
    pub fn is_client(&self) -> bool {
        self.realm() == Some(Realm::Client)
    }
    pub fn is_menu(&self) -> bool {
        self.realm() == Some(Realm::Menu)
    }
}

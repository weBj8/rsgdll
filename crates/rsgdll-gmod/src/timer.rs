//! `timer` library wrappers (spec §10.3). Named timers are tracked and removed at close.

use std::time::Duration;

use rsgdll_core::conv::LuaFunction;
use rsgdll_core::{Lua, Result, anyhow, tracked};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Forever,
    Times(u32),
}

impl Repeat {
    fn reps(self) -> u32 {
        match self {
            Repeat::Forever => 0,
            Repeat::Times(n) => n.max(1),
        }
    }
}

fn timer_fn(lua: &Lua, name: &str) -> Result<LuaFunction> {
    lua.global::<Option<LuaFunction>>(&format!("timer.{name}"))?.ok_or_else(|| anyhow!("timer.{name} is not available"))
}

fn closure(lua: &Lua, f: impl Fn(&Lua) -> Result<()> + 'static) -> Result<LuaFunction> {
    lua.create_function(move |lua, ()| f(lua))
}

/// `timer.Create(name, delay, reps, f)`.
pub fn create(lua: &Lua, name: &str, delay: Duration, repeat: Repeat, f: impl Fn(&Lua) -> Result<()> + 'static) -> Result<()> {
    let cb = closure(lua, f)?;
    timer_fn(lua, "Create")?.call::<_, ()>(lua, (name, delay.as_secs_f64(), repeat.reps(), cb))?;
    tracked::timer(lua, name);
    Ok(())
}

/// `timer.Simple(delay, f)`.
pub fn simple(lua: &Lua, delay: Duration, f: impl Fn(&Lua) -> Result<()> + 'static) -> Result<()> {
    let cb = closure(lua, f)?;
    timer_fn(lua, "Simple")?.call(lua, (delay.as_secs_f64(), cb))
}

/// `timer.Adjust(name, delay, reps)`; returns whether the timer exists.
pub fn adjust(lua: &Lua, name: &str, delay: Duration, repeat: Repeat) -> Result<bool> {
    timer_fn(lua, "Adjust")?.call::<_, Option<bool>>(lua, (name, delay.as_secs_f64(), repeat.reps())).map(|b| b.unwrap_or(false))
}

pub fn remove(lua: &Lua, name: &str) -> Result<()> {
    timer_fn(lua, "Remove")?.call::<_, ()>(lua, name)?;
    tracked::untrack_timer(lua, name);
    Ok(())
}

pub fn exists(lua: &Lua, name: &str) -> Result<bool> {
    timer_fn(lua, "Exists")?.call::<_, Option<bool>>(lua, name).map(|b| b.unwrap_or(false))
}

pub fn pause(lua: &Lua, name: &str) -> Result<bool> {
    timer_fn(lua, "Pause")?.call::<_, Option<bool>>(lua, name).map(|b| b.unwrap_or(false))
}

pub fn unpause(lua: &Lua, name: &str) -> Result<bool> {
    timer_fn(lua, "UnPause")?.call::<_, Option<bool>>(lua, name).map(|b| b.unwrap_or(false))
}

pub fn toggle(lua: &Lua, name: &str) -> Result<bool> {
    timer_fn(lua, "Toggle")?.call::<_, Option<bool>>(lua, name).map(|b| b.unwrap_or(false))
}

/// Parses `"1s"`, `"500ms"`, `"2m"`, `"1.5s"` (used by `#[rsgdll::timer]`).
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    let secs = match unit {
        "ms" => n / 1000.0,
        "s" => n,
        "m" => n * 60.0,
        "h" => n * 3600.0,
        _ => return None,
    };
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("1s"), Some(Duration::from_secs(1)));
        assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration("2m"), Some(Duration::from_secs(120)));
        assert_eq!(parse_duration("x"), None);
        assert_eq!(parse_duration("1d"), None);
    }
}

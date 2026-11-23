//! The script limits in the settings file: `script.fuel`, `script.memory_mb`,
//! `script.call_depth` and `script.growth_mb`. Written only when changed from the defaults,
//! so a file of someone who never changed them stays as it was; a value out of range is
//! brought into range. The script's memory and the document growth are separate settings.

use pikslar_bind::ScriptLimits;

/// Megabytes as bytes, held below a size no setting reaches so the shift cannot overflow.
fn mib(mb: u64) -> usize {
    usize::try_from(mb.min(1 << 20)).unwrap_or(usize::MAX) << 20
}

/// Reads one settings line about script limits; any other line leaves them as they were.
pub(crate) fn parse(name: &str, value: &str, limits: ScriptLimits) -> ScriptLimits {
    let number = value.parse::<u64>().ok();
    let read = match (name, number) {
        ("script.fuel", Some(fuel)) => ScriptLimits { fuel, ..limits },
        ("script.memory_mb", Some(mb)) => ScriptLimits {
            memory: mib(mb),
            ..limits
        },
        ("script.call_depth", Some(depth)) => ScriptLimits {
            call_depth: u32::try_from(depth).unwrap_or(u32::MAX),
            ..limits
        },
        ("script.growth_mb", Some(mb)) => ScriptLimits {
            growth: mib(mb),
            ..limits
        },
        _ => limits,
    };
    read.clamped()
}

/// The lines for limits changed from the defaults.
pub(crate) fn lines(limits: ScriptLimits) -> Vec<String> {
    let d = ScriptLimits::DEFAULT;
    let mut out = Vec::new();
    if limits.fuel != d.fuel {
        out.push(format!("script.fuel = {}", limits.fuel));
    }
    if limits.memory != d.memory {
        out.push(format!("script.memory_mb = {}", limits.memory >> 20));
    }
    if limits.call_depth != d.call_depth {
        out.push(format!("script.call_depth = {}", limits.call_depth));
    }
    if limits.growth != d.growth {
        out.push(format!("script.growth_mb = {}", limits.growth >> 20));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn defaults_write_nothing() {
        assert!(lines(ScriptLimits::DEFAULT).is_empty());
        assert!(!Settings::default().to_text().contains("script."));
        assert_eq!(Settings::default().script, ScriptLimits::DEFAULT);
    }

    #[test]
    fn changed_limits_are_kept_through_the_file() {
        let s = Settings {
            script: ScriptLimits {
                fuel: 1_000_000,
                memory: 64 << 20,
                call_depth: 32,
                growth: 512 << 20,
            },
            ..Settings::default()
        };
        let text = s.to_text();
        assert!(text.contains("script.memory_mb = 64"), "{text}");
        assert!(text.contains("script.growth_mb = 512"), "{text}");
        assert_eq!(Settings::parse(&text), s);
    }

    #[test]
    fn bad_or_wild_values_keep_a_usable_limit() {
        let read = Settings::parse(
            "script.fuel = lots\nscript.memory_mb = 99999999999\nscript.call_depth = 0\n\
             script.growth_mb = 99999999999\n",
        );
        assert_eq!(read.script.fuel, ScriptLimits::DEFAULT.fuel);
        assert_eq!(read.script.memory, ScriptLimits::MEMORY_RANGE.1);
        assert_eq!(read.script.call_depth, 1);
        assert_eq!(read.script.growth, ScriptLimits::GROWTH_RANGE.1);
        let low = Settings::parse("script.growth_mb = 0\n");
        assert_eq!(low.script.growth, ScriptLimits::GROWTH_RANGE.0);
        let junk = Settings::parse("script.growth_mb = -5\n");
        assert_eq!(junk.script.growth, ScriptLimits::DEFAULT.growth);
    }

    #[test]
    fn memory_and_growth_are_set_apart() {
        let mem = Settings::parse("script.memory_mb = 128\n").script;
        assert_eq!((mem.memory, mem.growth), (128 << 20, 256 << 20));
        assert_eq!(lines(mem), vec!["script.memory_mb = 128".to_string()]);
        let grown = Settings::parse("script.growth_mb = 64\n").script;
        assert_eq!((grown.memory, grown.growth), (16 << 20, 64 << 20));
        assert_eq!(lines(grown), vec!["script.growth_mb = 64".to_string()]);
    }
}

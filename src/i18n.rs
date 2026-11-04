//! Interface languages. Text is written in English in the code; `tr` swaps in the German or
//! French version from the table in `i18n_table.rs` (English when a text has no translation).
//! Texts with numbers or names in them use `tf!("… {name} …", name = value)`.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// (setting value, name shown in the language list)
pub const LANGUAGES: [(&str, &str); 4] = [("", "Same as system"), ("en", "English"), ("de", "Deutsch"), ("fr", "Français")];

/// 0 English, 1 German, 2 French.
static LANG: AtomicU8 = AtomicU8::new(0);

/// Use the language from the settings ("" = the system's).
pub fn set(code: &str) {
    let code = if code.is_empty() { system() } else { code.to_string() };
    LANG.store(match code.get(..2) { Some("de") => 1, Some("fr") => 2, _ => 0 }, Ordering::Relaxed);
}

/// The system's language code ("de", "fr", …), or "en".
fn system() -> String {
    #[cfg(target_os = "android")]
    return crate::droid::language().unwrap_or_default();
    #[cfg(windows)]
    {
        let mut buf = [0u16; 85];
        let n = unsafe { windows_sys::Win32::Globalization::GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 1 { return String::from_utf16_lossy(&buf[..(n - 1) as usize]).to_lowercase(); }
    }
    #[allow(unreachable_code)]
    ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty() && v != "C" && v != "POSIX").unwrap_or_default().to_lowercase()
}

/// The interface language in use: "en", "de" or "fr".
pub fn code() -> &'static str { ["en", "de", "fr"][LANG.load(Ordering::Relaxed) as usize] }

/// Added to AI instructions so answers come back in the interface language.
pub fn reply_hint() -> &'static str {
    match code() { "de" => " Write your reply in German.", "fr" => " Write your reply in French.", _ => "" }
}

fn table() -> &'static HashMap<&'static str, [&'static str; 2]> {
    static T: OnceLock<HashMap<&'static str, [&'static str; 2]>> = OnceLock::new();
    T.get_or_init(|| crate::i18n_table::TABLE.iter().map(|(en, de, fr)| (*en, [*de, *fr])).collect())
}

/// The text in the chosen language.
pub fn tr(en: &str) -> &str {
    match LANG.load(Ordering::Relaxed) {
        0 => en,
        l => table().get(en).map_or(en, |t| t[l as usize - 1]),
    }
}

/// Put values into a translated template: `{name}` becomes the value given for `name`.
pub fn fill(template: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = template.to_string();
    for (k, v) in args { out = out.replace(&format!("{{{k}}}"), &v.to_string()); }
    out
}

/// `format!` for interface text: `tf!("{n} cards", n = count)` is translated first, then filled in.
#[macro_export]
macro_rules! tf {
    ($t:literal $(, $k:ident = $v:expr)* $(,)?) => { $crate::i18n::fill($crate::i18n::tr($t), &[$((stringify!($k), &$v as &dyn std::fmt::Display)),*]) };
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    /// Tests that switch the (global) interface language take turns.
    pub(crate) static LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn translates_and_fills() {
        let _turn = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set("de");
        assert_eq!(tr("Settings"), "Einstellungen");
        assert_eq!(tr("no such text"), "no such text");
        assert_eq!(crate::tf!("Review due ({due_all})", due_all = 3), "Fällige wiederholen (3)");
        set("en");
        assert_eq!(crate::tf!("Review due ({due_all})", due_all = 3), "Review due (3)");
        // every translation keeps the {placeholders} of its English text
        let ph = |s: &str| { let mut v: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|x| x.0.to_string())).collect(); v.sort(); v };
        for (en, de, fr) in crate::i18n_table::TABLE { assert!(ph(en) == ph(de) && ph(en) == ph(fr), "placeholders differ: {en}"); }
    }
}

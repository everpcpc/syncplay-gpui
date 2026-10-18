use gpui_kit::SharedString;

/// (locale code, native name) pairs shown in the settings language picker.
pub const LANGUAGES: [(&str, &str); 4] = [
    ("en", "English"),
    ("zh-CN", "简体中文"),
    ("zh-TW", "繁體中文"),
    ("ja", "日本語"),
];

pub fn apply_language(language: &str) {
    rust_i18n::set_locale(language);
}

/// Lookup for keys held in data (settings option tables), where the `t!`
/// macro's literal-key requirement cannot be satisfied.
pub fn tr(key: &str) -> SharedString {
    crate::_rust_i18n_translate(&rust_i18n::locale(), key)
        .into_owned()
        .into()
}

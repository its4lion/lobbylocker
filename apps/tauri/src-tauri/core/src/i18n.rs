//! Data-driven JSON locales with an English fallback and locale-provided LTR/RTL direction.
use crate::{model::validate_id, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

/// A translation file; its code matches its filename and the saved language setting.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Locale {
    /// BCP-47 language tag, such as `en`, `ar`, or `pt-BR`.
    pub code: String,
    /// Native language name displayed in the language selector.
    pub name: String,
    /// Either `ltr` or `rtl`.
    pub direction: String,
    /// Message IDs map to translated labels or templates.
    pub messages: BTreeMap<String, String>,
}

/// Cheap shared view of the selected locale and its English fallback.
#[derive(Clone)]
pub struct Translator {
    locale: Arc<Locale>,
    fallback: Arc<Locale>,
}
impl Translator {
    /// Looks up a message, falling back to English and finally to the message ID.
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.locale
            .messages
            .get(key)
            .or_else(|| self.fallback.messages.get(key))
            .map(String::as_str)
            .unwrap_or(key)
    }
    /// The actual selected language tag, after fallback.
    pub fn code(&self) -> &str {
        &self.locale.code
    }
    /// Reading direction defined in the locale file.
    pub fn direction(&self) -> &str {
        &self.locale.direction
    }
    /// Substitutes named placeholders without interpreting markup or nested placeholders.
    pub fn format(&self, key: &str, variables: &[(&str, &str)]) -> String {
        let template = self.text(key);
        let mut result = String::new();
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            result.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}') else {
                result.push_str(&rest[start..]);
                return result;
            };
            let name = &rest[start + 1..start + end];
            let token = &rest[start..start + end + 1];
            result.push_str(
                variables
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| *value)
                    .unwrap_or(token),
            );
            rest = &rest[start + end + 1..];
        }
        result.push_str(rest);
        result
    }
}

/// Installed locale files, with a built-in English fallback for incomplete translations.
pub struct Catalogue {
    /// All available languages, including bundled English and Arabic.
    pub locales: Vec<Arc<Locale>>,
    /// Invalid additional locale files do not prevent startup.
    pub warnings: Vec<String>,
    fallback: Arc<Locale>,
}
fn parse(input: &str) -> Result<Locale> {
    let locale: Locale = serde_json::from_str(input).map_err(|e| e.to_string())?;
    validate_id(&locale.code)?;
    if locale.name.trim().is_empty()
        || locale.name.len() > 120
        || !matches!(locale.direction.as_str(), "ltr" | "rtl")
        || locale.messages.len() > 3000
    {
        return Err("Invalid locale name, direction, or message count.".into());
    }
    Ok(locale)
}
impl Catalogue {
    /// Loads bundled English/Arabic plus valid JSON locales from `directory`.
    /// Errors indicate invalid bundled defaults; external-file errors become warnings.
    pub fn load(directory: &Path) -> Result<Self> {
        let english = Arc::new(parse(include_str!("../../../../../data/locales/en.json"))?);
        let arabic = Arc::new(parse(include_str!("../../../../../data/locales/ar.json"))?);
        let mut locales = BTreeMap::from([
            (english.code.clone(), english),
            (arabic.code.clone(), arabic),
        ]);
        let mut warnings = Vec::new();
        if directory.is_dir() {
            match fs::read_dir(directory) {
                Ok(entries) => {
                    for entry in entries.take(1000) {
                        let entry = match entry {
                            Ok(e) => e,
                            Err(e) => {
                                warnings.push(e.to_string());
                                continue;
                            }
                        };
                        let path = entry.path();
                        if path.extension().is_none_or(|e| e != "json") {
                            continue;
                        }
                        let read: Result<Locale> = (|| {
                            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 2_000_000 {
                                return Err("Locale exceeds 2 MB.".into());
                            }
                            let locale =
                                parse(&fs::read_to_string(&path).map_err(|e| e.to_string())?)?;
                            if path.file_stem().and_then(|s| s.to_str())
                                != Some(locale.code.as_str())
                            {
                                return Err("Locale filename must match its code.".into());
                            }
                            Ok(locale)
                        })();
                        match read {
                            Ok(locale) => {
                                locales.insert(locale.code.clone(), Arc::new(locale));
                            }
                            Err(e) => warnings.push(format!("{}: {e}", path.display())),
                        }
                    }
                }
                Err(e) => warnings.push(e.to_string()),
            }
        }
        let fallback = locales
            .get("en")
            .cloned()
            .ok_or("English locale unavailable.")?;
        Ok(Self {
            locales: locales.into_values().collect(),
            warnings,
            fallback,
        })
    }
    /// Returns a translator; unknown language codes select English.
    pub fn translator(&self, code: &str) -> Translator {
        let locale = self
            .locales
            .iter()
            .find(|l| l.code == code)
            .cloned()
            .unwrap_or_else(|| Arc::clone(&self.fallback));
        Translator {
            locale,
            fallback: Arc::clone(&self.fallback),
        }
    }
}
#[cfg(test)]
#[path = "../tests/unit/i18n.rs"]
mod tests;

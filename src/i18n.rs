use std::{collections::HashMap, fs, path::Path};

use toml::Value;

type Translations = HashMap<String, HashMap<String, String>>;

pub struct TranslationStore {
    translations: Translations,
    language_names: Vec<(String, String)>,
}

impl TranslationStore {
    pub fn new() -> Self {
        let mut translations = HashMap::new();
        let mut language_names = Vec::new();

        let lang_dir = Path::new("lang/");
        let entries = fs::read_dir(lang_dir)
            .unwrap_or_else(|_| panic!("Failed to read language directory: {:?}", lang_dir));

        for entry in entries.flatten() {
            let path = entry.path();
            let lang = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_else(|| panic!("Invalid file name in lang directory: {:?}", path));

            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("Failed to read language file: {:?}", path));

            let parsed: Value = toml::from_str(&contents)
                .unwrap_or_else(|e| panic!("Failed to parse TOML in file {:?}: {:?}", path, e));

            let table = parsed.as_table().expect("Expected a table at root of TOML");

            let lang_translations: HashMap<String, String> = table
                .iter()
                .filter_map(|(key, val)| val.as_str().map(|s| (key.clone(), s.to_string())))
                .collect();

            if lang_translations.len() != table.len() {
                panic!("Incomplete or invalid translation in file: {:?}", path);
            }

            let lang_name = table
                .get("language_name")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("Missing 'language_name' in file: {:?}", path));

            language_names.push((lang.to_string(), lang_name.to_string()));
            println!("Loaded language {}", lang);
            translations.insert(lang.to_string(), lang_translations);
        }

        language_names.sort_by_key(|value| value.0.clone());

        Self {
            translations,
            language_names,
        }
    }

    pub fn get_translation(&self, lang: &str) -> &HashMap<String, String> {
        self.translations.get(lang).unwrap_or_else(|| {
            self.translations
                .get("en")
                .expect("English fallback translation not found")
        })
    }

    pub fn available_languages(&self) -> &Vec<(String, String)> {
        &self.language_names
    }
}

//! Translations.
//!
//! **Files.** A language is `lang/<code>.json` (codes like `es-ES`, `zh-Hant`): one flat JSON object of
//! `"key": "translated text"`, plus an optional `"_meta"` object:
//!
//! ```json
//! { "_meta": { "name": "Español (México)", "group": "Español", "fallback": "es-ES", "cjk": "zh-Hant" },
//!   "Worlds": "Mundos", "block.stone": "Piedra" }
//! ```
//!
//! * `name` is how the language is listed (in its own language); `group` is the heading several regional
//!   variants share in the picker (a group with one member is listed directly);
//!   `fallback` is the language tried next for a missing string (before English); `cjk` picks the
//!   regional ideograph set (`zh-Hans`, `zh-Hant`, `ja`).
//! * **Keys.** For interface text the key *is* the English text (`"Worlds"`), so code reads
//!   `tr("Worlds")` and always has a sensible fallback; named things use `block.<id>`, `item.<id>`, and so on.
//!   `{name}` is a placeholder (`tr_args`). Plurals are `key.one` / `key.other` (`tr_n`), chosen by the
//!   language's own rule.
//! * An **empty string means "not translated yet"**: it is skipped when looking up and when counting.
//!
//! **English is the reference.** `lang/en.json` lists every key; a language's percentage is the share of
//! those keys it has a non-empty translation for, so it drops by itself when keys are added and rises as
//! they are translated.
//!
//! **Mods** add `lang/<code>.json` to their folder. A file for a language the game has adds to it (a mod
//! translating its own content, or *another mod's* - the keys are the other mod's names); a file for a code
//! the game doesn't know, with a `_meta`, adds a new language to the list. A mod should ship `lang/en.json`
//! for its own keys so the percentages count them. Mods load after the game and later mods after earlier
//! ones, each overriding what came before.
//!
//! **Commands stay English**: `/give`, its arguments and its syntax are never translated (their descriptions
//! and results are).
//!
//! The active language is process-wide (`install`) so spawn functions anywhere can call [`tr`] without a
//! resource in their signature - the same reasoning as `gui_skin`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};

use bevy::prelude::*;
use serde::Deserialize;

pub const FOLDER: &str = "lang";
pub const ENGLISH: &str = "en";
const META: &str = "_meta";
/// How far a `fallback` chain is followed (and so how a cycle ends).
const MAX_CHAIN: usize = 4;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Meta {
    pub name: Option<String>,
    pub group: Option<String>,
    pub fallback: Option<String>,
    pub cjk: Option<String>,
    /// Written right to left (Arabic). Recorded for the interface to use; text itself is laid out by the text
    /// engine's bidi handling either way.
    pub rtl: Option<bool>,
}

/// One parsed `lang/<code>.json`.
#[derive(Clone, Debug)]
pub struct LangFile {
    pub code: String,
    pub meta: Option<Meta>,
    pub entries: HashMap<String, String>,
}

/// Reads one language file's text. Non-text values are skipped, with a note.
pub fn parse_file(code: &str, text: &str) -> Result<(LangFile, Vec<String>), String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{code}: {e}"))?;
    let serde_json::Value::Object(map) = value else { return Err(format!("{code}: not a JSON object")) };
    let (mut entries, mut notes, mut meta) = (HashMap::new(), Vec::new(), None);
    for (k, v) in map {
        if k == META {
            meta = serde_json::from_value::<Meta>(v).map_err(|e| notes.push(format!("{code}: bad _meta: {e}"))).ok();
            continue;
        }
        match v {
            serde_json::Value::String(s) => {
                entries.insert(k, s);
            }
            _ => notes.push(format!("{code}: {k:?} is not text")),
        }
    }
    Ok((LangFile { code: code.to_string(), meta, entries }, notes))
}

#[derive(Clone, Debug)]
pub struct Language {
    pub code: String,
    pub name: String,
    pub group: String,
    pub fallback: Option<String>,
    pub cjk: Option<String>,
    pub rtl: bool,
    pub strings: HashMap<String, String>,
}

/// A heading in the picker: one language, or several regional variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Catalog {
    langs: BTreeMap<String, Language>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self::build(Vec::new())
    }
}

impl Catalog {
    /// Merges files in order: the first file with a `_meta` names a language, strings are overridden by later files.
    pub fn build(files: Vec<LangFile>) -> Self {
        let mut langs: BTreeMap<String, Language> = BTreeMap::new();
        langs.insert(
            ENGLISH.into(),
            Language { code: ENGLISH.into(), name: "English (United States)".into(), group: "English".into(), fallback: None, cjk: None, rtl: false, strings: HashMap::new() },
        );
        for f in files {
            let lang = langs.entry(f.code.clone()).or_insert_with(|| Language {
                code: f.code.clone(),
                name: f.code.clone(),
                group: f.code.clone(),
                fallback: None,
                cjk: None,
                rtl: false,
                strings: HashMap::new(),
            });
            if let Some(m) = &f.meta {
                // A language is named once, by whoever got there first (the game, before any mod).
                if lang.name == lang.code {
                    if let Some(n) = &m.name {
                        lang.name = n.clone();
                        lang.group = m.group.clone().unwrap_or_else(|| n.clone());
                    }
                }
                lang.fallback = lang.fallback.take().or_else(|| m.fallback.clone());
                lang.cjk = lang.cjk.take().or_else(|| m.cjk.clone());
                lang.rtl |= m.rtl.unwrap_or(false);
            }
            lang.strings.extend(f.entries);
        }
        Catalog { langs }
    }

    pub fn language(&self, code: &str) -> Option<&Language> {
        self.langs.get(code)
    }

    pub fn contains(&self, code: &str) -> bool {
        self.langs.contains_key(code)
    }

    /// Every key English defines with text: what a translation is measured against.
    pub fn english_keys(&self) -> Vec<&str> {
        self.langs[ENGLISH].strings.iter().filter(|(_, v)| !v.is_empty()).map(|(k, _)| k.as_str()).collect()
    }

    /// How much of the game `code` covers, as a whole percent (rounded down, so 99.9% never reads 100).
    pub fn percent(&self, code: &str) -> u8 {
        let keys = self.english_keys();
        // English and its dialects (`en-GB`...) are the reference text itself: nothing is left to translate.
        if code == ENGLISH || code.starts_with("en-") || keys.is_empty() {
            return 100;
        }
        if !self.langs.contains_key(code) {
            return 0;
        }
        // Counted the way a player sees it: a string a variant inherits from its fallback language is translated.
        let done = keys.iter().filter(|k| self.lookup_translated(code, k).is_some()).count();
        ((done * 100) / keys.len()) as u8
    }

    /// `lookup`, but never English: `None` when only English has the text.
    fn lookup_translated(&self, code: &str, key: &str) -> Option<&str> {
        let mut at = Some(code);
        for _ in 0..MAX_CHAIN {
            let lang = at.and_then(|c| self.langs.get(c))?;
            if let Some(s) = lang.strings.get(key).filter(|s| !s.is_empty()) {
                return Some(s);
            }
            at = lang.fallback.as_deref();
        }
        None
    }

    /// The text for `key` in `code`: its own, then each fallback's, then English's. `None` if nobody has it.
    pub fn lookup(&self, code: &str, key: &str) -> Option<&str> {
        let mut at = Some(code);
        for _ in 0..MAX_CHAIN {
            let Some(lang) = at.and_then(|c| self.langs.get(c)) else { break };
            if let Some(s) = lang.strings.get(key).filter(|s| !s.is_empty()) {
                return Some(s);
            }
            at = lang.fallback.as_deref();
        }
        self.langs[ENGLISH].strings.get(key).map(String::as_str).filter(|s| !s.is_empty())
    }

    /// The picker's headings: English first, then by name; variants of one group together.
    pub fn groups(&self) -> Vec<Group> {
        let mut by_group: BTreeMap<&str, Vec<&Language>> = BTreeMap::new();
        for l in self.langs.values() {
            by_group.entry(l.group.as_str()).or_default().push(l);
        }
        let mut out: Vec<Group> = by_group
            .into_iter()
            .map(|(name, mut ls)| {
                ls.sort_by(|a, b| a.name.cmp(&b.name));
                Group { name: name.to_string(), members: ls.iter().map(|l| l.code.clone()).collect() }
            })
            .collect();
        out.sort_by(|a, b| (a.name != "English").cmp(&(b.name != "English")).then(a.name.cmp(&b.name)));
        out
    }
}

/// `zero`, `one`, `two`, `few`, `many` or `other`, by the language's own rule (the base language of the code decides).
/// Only Arabic needs more than `one`/`other` so far; a language file gives the forms its rule can produce
/// (`key.one`, `key.two`...) and `key.other` is what every language must have.
pub fn plural_category(code: &str, n: i64) -> &'static str {
    let base = code.split('-').next().unwrap_or(code);
    if base == "ar" {
        // CLDR: 0 zero, 1 one, 2 two, 3-10 few (by the last two digits), 11-99 many, the rest other.
        let m = n.unsigned_abs() % 100;
        return match n.unsigned_abs() {
            0 => "zero",
            1 => "one",
            2 => "two",
            _ if (3..=10).contains(&m) => "few",
            _ if (11..=99).contains(&m) => "many",
            _ => "other",
        };
    }
    let one = match base {
        // No plural forms.
        "zh" | "yue" | "ja" | "ko" | "vi" | "th" | "id" => false,
        // 0 and 1 are singular; Portugal's Portuguese only 1.
        "fr" | "hi" => (0..=1).contains(&n),
        "pt" => if code == "pt-PT" { n == 1 } else { (0..=1).contains(&n) },
        _ => n == 1,
    };
    if one { "one" } else { "other" }
}

// ---------------------------------------------------------------------------
// The active language
// ---------------------------------------------------------------------------

struct Active {
    catalog: Arc<Catalog>,
    code: String,
}

static ACTIVE: RwLock<Option<Active>> = RwLock::new(None);

/// Makes `code` the language every [`tr`] call reads, from `catalog`.
pub fn install(catalog: Arc<Catalog>, code: &str) {
    *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = Some(Active { catalog, code: code.to_string() });
}

/// The code of the language in use (`en` before one is installed).
pub fn active_code() -> String {
    ACTIVE.read().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(ENGLISH.to_string(), |a| a.code.clone())
}

/// Looks `key` up in the active language, or gives `fallback` when no language has it.
pub fn tr_or(key: &str, fallback: &str) -> String {
    let guard = ACTIVE.read().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().and_then(|a| a.catalog.lookup(&a.code, key)).unwrap_or(fallback).to_string()
}

/// Interface text: the key is the English text, which is also what shows when nothing translates it.
pub fn tr(key: &str) -> String {
    tr_or(key, key)
}

/// `tr` with `{name}` placeholders filled in.
pub fn tr_args(key: &str, args: &[(&str, &str)]) -> String {
    fill(&tr(key), args)
}

/// Replaces each `{name}` with its value.
pub fn fill(text: &str, args: &[(&str, &str)]) -> String {
    let mut out = text.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// A count in the active language's plural form: `base.one` or `base.other`, with `{n}` filled in. The English
/// forms are passed in, so English is right even when no language file is loaded.
pub fn tr_n(base: &str, n: i64, one_en: &str, other_en: &str, args: &[(&str, &str)]) -> String {
    let category = plural_category(&active_code(), n);
    let english = if plural_category(ENGLISH, n) == "one" { one_en } else { other_en };
    let text = tr_or(&format!("{base}.{category}"), &tr_or(&format!("{base}.other"), english));
    let count = n.to_string();
    let mut all = vec![("n", count.as_str())];
    all.extend_from_slice(args);
    fill(&text, &all)
}

/// A name from content data: `block.stone`, `item.twig`... or the English `fallback` the game derives from the id.
pub fn name_of(kind: &str, id: &str, fallback: &str) -> String {
    tr_or(&format!("{kind}.{id}"), fallback)
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

fn read_dir_files(dir: &Path, notes: &mut Vec<String>) -> Vec<LangFile> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        let Some(code) = p.file_stem().and_then(|s| s.to_str()) else { continue };
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| parse_file(code, &t)) {
            Ok((f, n)) => {
                notes.extend(n);
                out.push(f);
            }
            Err(e) => notes.push(format!("{}: {e}", p.display())),
        }
    }
    out
}

/// The game's languages, then every installed mod's (sorted by folder, so the result doesn't depend on the
/// file system's listing order). The notes are problems found, for a log or the Language screen.
pub fn load_catalog(game_dir: &Path, installed_mods: &Path) -> (Catalog, Vec<String>) {
    let mut notes = Vec::new();
    let mut files = read_dir_files(game_dir, &mut notes);
    let (mut mods, _) = crate::mods::scan_all(installed_mods);
    mods.sort_by(|a, b| a.folder.cmp(&b.folder));
    for m in mods {
        files.extend(read_dir_files(&m.path.join(FOLDER), &mut notes));
    }
    (Catalog::build(files), notes)
}

/// The catalog and the choice, as a resource the settings screen reads and changes.
#[derive(Resource, Clone)]
pub struct Languages {
    pub catalog: Arc<Catalog>,
    pub code: String,
    pub notes: Vec<String>,
}

impl Default for Languages {
    fn default() -> Self {
        Self { catalog: Arc::new(Catalog::default()), code: ENGLISH.into(), notes: Vec::new() }
    }
}

impl Languages {
    /// A language as the picker lists it: its own name, then its name in the language in use - "Español (España) /
    /// Spanish (Spain)" while English is chosen. The second part is left out when it is the same (Spanish while
    /// Spanish is in use) or when nobody has named the language (a mod's new one: add `lang.name.<code>`).
    pub fn display_name(&self, code: &str) -> String {
        let native = self.catalog.language(code).map_or(code.to_string(), |l| l.name.clone());
        both(&native, self.catalog.lookup(&self.code, &format!("lang.name.{code}")))
    }

    /// The same for a group heading (`Español / Spanish`).
    pub fn display_group(&self, group: &str) -> String {
        both(group, self.catalog.lookup(&self.code, &format!("lang.group.{group}")))
    }

    /// Switches to `code` if it exists; returns whether it changed. [`apply_language`] installs it for [`tr`].
    pub fn select(&mut self, code: &str) -> bool {
        if !self.catalog.contains(code) || self.code == code {
            return false;
        }
        self.code = code.to_string();
        true
    }

    /// Re-reads the files (a mod was added) and keeps the choice if it still exists.
    pub fn reload(&mut self, game_dir: &Path, installed_mods: &Path) {
        let (catalog, notes) = load_catalog(game_dir, installed_mods);
        self.catalog = Arc::new(catalog);
        self.notes = notes;
        if !self.catalog.contains(&self.code) {
            self.code = ENGLISH.into();
        }
    }

    /// The ideograph set the language asks for, if it names one.
    pub fn cjk_style(&self) -> Option<crate::cjk::Style> {
        match self.catalog.language(&self.code)?.cjk.as_deref()? {
            "zh-Hans" => Some(crate::cjk::Style::Chinese),
            "zh-Hant" => Some(crate::cjk::Style::TraditionalChinese),
            "ja" => Some(crate::cjk::Style::Japanese),
            _ => None,
        }
    }
}

/// A name next to its translation, once if they are the same. When one of them is written right to left
/// (Arabic) each is wrapped in a Unicode isolate, so the two stay in their own runs instead of the bidi algorithm
/// shuffling the Latin and Arabic words of one line into each other.
fn both(native: &str, translated: Option<&str>) -> String {
    match translated {
        Some(t) if !t.is_empty() && t != native => {
            if has_rtl(native) || has_rtl(t) {
                format!("\u{2068}{native}\u{2069} / \u{2068}{t}\u{2069}")
            } else {
                format!("{native} / {t}")
            }
        }
        _ => native.to_string(),
    }
}

/// Whether `s` has any character from a right-to-left script (Hebrew, Arabic, Syriac, Thaana, N'Ko...).
fn has_rtl(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF))
}

pub struct LanguagePlugin;

impl Plugin for LanguagePlugin {
    fn build(&self, app: &mut App) {
        let store = app.world().get_resource::<crate::save::SaveStore>().cloned();
        let installed = app.world().get_resource::<crate::mods::InstalledMods>().map(|m| m.0.clone()).unwrap_or_else(|| crate::mods::InstalledMods::default().0);
        let (catalog, notes) = load_catalog(&crate::blocks::find_data_dir("lang"), &installed);
        // `CRAFT_LANG=<code>` overrides the saved choice for one run (screenshots, testing a translation).
        let wanted = std::env::var("CRAFT_LANG").ok().or_else(|| store.map(|s| s.load_language())).unwrap_or_else(|| ENGLISH.into());
        let code = if catalog.contains(&wanted) { wanted } else { ENGLISH.into() };
        let catalog = Arc::new(catalog);
        install(catalog.clone(), &code);
        for n in &notes {
            warn!("language files: {n}");
        }
        let languages = Languages { catalog, code, notes };
        if let Some(style) = languages.cjk_style() {
            app.insert_resource(crate::cjk::CjkStyle(style));
        }
        app.insert_resource(languages).add_systems(PreUpdate, apply_language.run_if(resource_changed::<Languages>));
    }
}

/// Makes the choice in [`Languages`] the one [`tr`] reads (and the ideograph set it asks for the one drawn).
pub fn apply_language(languages: Res<Languages>, cjk: Option<ResMut<crate::cjk::CjkStyle>>) {
    install(languages.catalog.clone(), &languages.code);
    if let (Some(style), Some(mut cjk)) = (languages.cjk_style(), cjk) {
        if cjk.0 != style {
            cjk.0 = style;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn file(code: &str, json: &str) -> LangFile {
        parse_file(code, json).unwrap().0
    }

    fn catalog(files: &[(&str, &str)]) -> Catalog {
        Catalog::build(files.iter().map(|(c, j)| file(c, j)).collect())
    }

    #[test]
    fn a_file_reads_its_meta_and_strings_and_notes_what_is_not_text() {
        let (f, notes) = parse_file("es", r#"{"_meta": {"name": "Español", "fallback": "en"}, "Hi": "Hola", "n": 3}"#).unwrap();
        assert_eq!(f.meta.unwrap().name.as_deref(), Some("Español"));
        assert_eq!(f.entries["Hi"], "Hola");
        assert!(!f.entries.contains_key("n"));
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(parse_file("x", "[1]").is_err());
        assert!(parse_file("x", "{ cut").is_err());
    }

    #[test]
    fn lookup_tries_the_language_then_its_fallbacks_then_english_and_skips_empty_strings() {
        let c = catalog(&[
            ("en", r#"{"A": "a", "B": "b", "C": "c", "D": "d"}"#),
            ("xx", r#"{"_meta": {"name": "X"}, "A": "xa", "B": ""}"#),
            ("yy", r#"{"_meta": {"name": "Y", "fallback": "xx"}, "C": "yc"}"#),
        ]);
        assert_eq!(c.lookup("yy", "C"), Some("yc"));
        assert_eq!(c.lookup("yy", "A"), Some("xa"), "from the fallback");
        assert_eq!(c.lookup("yy", "B"), Some("b"), "an empty string means not translated: English");
        assert_eq!(c.lookup("yy", "D"), Some("d"));
        assert_eq!(c.lookup("yy", "nothing"), None);
        assert_eq!(c.lookup("unknown", "A"), Some("a"), "an unknown language is English");
    }

    #[test]
    fn a_fallback_loop_ends() {
        let c = catalog(&[
            ("en", r#"{"A": "a"}"#),
            ("p", r#"{"_meta": {"name": "P", "fallback": "q"}}"#),
            ("q", r#"{"_meta": {"name": "Q", "fallback": "p"}}"#),
        ]);
        assert_eq!(c.lookup("p", "A"), Some("a"));
        assert_eq!(c.percent("p"), 0);
    }

    #[test]
    fn the_percentage_is_adaptive_it_counts_what_english_has_and_what_the_player_would_see() {
        let mut files = vec![
            ("en", r#"{"A": "a", "B": "b", "C": "c", "D": "d"}"#),
            ("xx", r#"{"_meta": {"name": "X"}, "A": "1", "B": "2", "C": ""}"#),
            ("xy", r#"{"_meta": {"name": "XY", "fallback": "xx"}, "D": "4"}"#),
        ];
        let c = catalog(&files);
        assert_eq!(c.percent("en"), 100);
        assert_eq!(c.percent("xx"), 50, "two of four; the empty one is not translated");
        assert_eq!(c.percent("xy"), 75, "what it inherits from xx counts");
        // A key added to English drops every percentage with no change to any translation.
        files[0] = ("en", r#"{"A": "a", "B": "b", "C": "c", "D": "d", "E": "e", "F": "f", "G": "g", "H": "h"}"#);
        let c = catalog(&files);
        assert_eq!(c.percent("xx"), 25);
        assert_eq!(c.percent("xy"), 37, "rounded down");
        // Translating the new ones brings it back up.
        files[1] = ("xx", r#"{"_meta": {"name": "X"}, "A": "1", "B": "2", "C": "3", "D": "4", "E": "5", "F": "6", "G": "7", "H": "8"}"#);
        assert_eq!(catalog(&files).percent("xx"), 100);
        // Keys English doesn't have are not counted.
        files[1] = ("xx", r#"{"_meta": {"name": "X"}, "Z": "z"}"#);
        assert_eq!(catalog(&files).percent("xx"), 0);
    }

    #[test]
    fn variants_group_under_one_heading_and_a_lone_language_is_its_own() {
        let c = catalog(&[
            ("es-ES", r#"{"_meta": {"name": "Español (España)", "group": "Español"}}"#),
            ("es-MX", r#"{"_meta": {"name": "Español (México)", "group": "Español"}}"#),
            ("fr-FR", r#"{"_meta": {"name": "Français (France)", "group": "Français"}}"#),
            ("hi", r#"{"_meta": {"name": "हिन्दी"}}"#),
        ]);
        let groups = c.groups();
        assert_eq!(groups[0].name, "English", "English first");
        let es = groups.iter().find(|g| g.name == "Español").unwrap();
        assert_eq!(es.members, vec!["es-ES".to_string(), "es-MX".to_string()]);
        assert_eq!(groups.iter().find(|g| g.name == "Français").unwrap().members.len(), 1);
        assert_eq!(groups.iter().find(|g| g.name == "हिन्दी").unwrap().members, vec!["hi".to_string()], "no group: its own name");
    }

    #[test]
    fn the_first_file_to_name_a_language_keeps_the_name_so_a_mod_cannot_rename_the_games_languages() {
        let c = catalog(&[
            ("es", r#"{"_meta": {"name": "Español"}, "A": "game"}"#),
            ("es", r#"{"_meta": {"name": "Castellano!"}, "A": "mod"}"#),
        ]);
        assert_eq!(c.language("es").unwrap().name, "Español");
        assert_eq!(c.lookup("es", "A"), Some("mod"), "but a later file's strings win");
    }

    #[test]
    fn a_dialect_can_name_a_new_thing_its_own_way_before_anyone_needs_it_to() {
        // The day an eggplant is added: English has the reference word, the British branch has its own, the
        // Indian one another - and everyone else (and every other language's fallback) gets the reference.
        let c = catalog(&[
            ("en", r#"{"block.eggplant": "Eggplant", "Done": "Done"}"#),
            ("en-GB", r#"{"_meta": {"name": "English (United Kingdom)", "group": "English"}, "block.eggplant": "Aubergine"}"#),
            ("en-IN", r#"{"_meta": {"name": "English (India)", "group": "English"}, "block.eggplant": "Brinjal"}"#),
            ("en-NZ", r#"{"_meta": {"name": "English (New Zealand)", "group": "English"}}"#),
        ]);
        assert_eq!(c.lookup("en-GB", "block.eggplant"), Some("Aubergine"));
        assert_eq!(c.lookup("en-IN", "block.eggplant"), Some("Brinjal"));
        assert_eq!(c.lookup("en-NZ", "block.eggplant"), Some("Eggplant"), "no word of its own yet");
        assert_eq!(c.lookup("en", "block.eggplant"), Some("Eggplant"));
        assert_eq!(c.lookup("en-GB", "Done"), Some("Done"), "everything else is inherited");
        // None of them are "behind": a dialect is the reference text plus its differences.
        for code in ["en", "en-GB", "en-IN", "en-NZ"] {
            assert_eq!(c.percent(code), 100, "{code}");
        }
        // They are separate entries in the list, under one heading.
        assert_eq!(c.groups().iter().find(|g| g.name == "English").unwrap().members.len(), 4);
    }

    #[test]
    fn a_name_in_a_right_to_left_script_is_kept_in_its_own_run() {
        assert_eq!(both("Español", Some("Spanish")), "Español / Spanish", "left to right: untouched");
        assert_eq!(both("العربية", Some("Arabic")), "\u{2068}العربية\u{2069} / \u{2068}Arabic\u{2069}");
        assert_eq!(both("Español", Some("الإسبانية")), "\u{2068}Español\u{2069} / \u{2068}الإسبانية\u{2069}");
        assert_eq!(both("العربية", Some("العربية")), "العربية", "the same name shows once");
        assert!(has_rtl("עברית") && has_rtl("ܐܠܗܐ") && !has_rtl("中文"));
    }

    #[test]
    fn plural_rules_follow_each_language() {
        assert_eq!(plural_category("en", 1), "one");
        assert_eq!(plural_category("en", 0), "other");
        assert_eq!(plural_category("es-ES", 1), "one");
        assert_eq!(plural_category("fr-FR", 0), "one", "French: 0 and 1");
        assert_eq!(plural_category("fr-FR", 2), "other");
        assert_eq!(plural_category("pt-BR", 0), "one");
        assert_eq!(plural_category("pt-PT", 0), "other", "Portugal: only 1");
        assert_eq!(plural_category("hi", 0), "one");
        assert_eq!(plural_category("zh-Hant", 1), "other", "Chinese has no plural");
        assert_eq!(plural_category("yue-Hant", 1), "other", "nor does Cantonese");
        // Arabic has six forms; the last two digits decide few and many.
        let forms: Vec<&str> = [0, 1, 2, 3, 10, 11, 99, 100, 102, 103, 111, 200].iter().map(|n| plural_category("ar", *n)).collect();
        assert_eq!(forms, ["zero", "one", "two", "few", "few", "many", "many", "other", "other", "few", "many", "other"]);
    }

    #[test]
    fn placeholders_fill_and_counts_use_the_english_forms_when_no_language_is_installed() {
        assert_eq!(fill("{a} and {b}", &[("a", "1"), ("b", "2")]), "1 and 2");
        assert_eq!(tr("not a key"), "not a key");
        assert_eq!(tr_args("Hi {name}", &[("name", "Sam")]), "Hi Sam");
        assert_eq!(tr_n("count.mods", 1, "{n} mod", "{n} mods", &[]), "1 mod");
        assert_eq!(tr_n("count.mods", 3, "{n} mod", "{n} mods", &[]), "3 mods");
        assert_eq!(name_of("block", "stone", "Stone"), "Stone");
    }

    static N: AtomicU64 = AtomicU64::new(0);
    struct Dir(std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn dir() -> Dir {
        let d = std::env::temp_dir().join(format!("craftmjne-lang-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn make_mod(root: &Path, folder: &str, id: &str, files: &[(&str, &str)]) {
        write(&root.join(folder).join("mod.json"), &format!(r#"{{"id": "{id}"}}"#));
        for (code, json) in files {
            write(&root.join(folder).join("lang").join(format!("{code}.json")), json);
        }
    }

    #[test]
    fn mods_translate_their_own_content_other_mods_content_and_add_whole_languages() {
        let game = dir();
        let mods = dir();
        write(&game.0.join("en.json"), r#"{"Play": "Play"}"#);
        write(&game.0.join("es-ES.json"), r#"{"_meta": {"name": "Español (España)", "group": "Español"}, "Play": "Jugar"}"#);
        // A mod: its own English keys, its own Spanish, a translation of ANOTHER mod's key, and a new language.
        make_mod(&mods.0, "granite", "granite", &[
            ("en", r#"{"block.granite:granite": "Granite"}"#),
            ("es-ES", r#"{"block.granite:granite": "Granito"}"#),
            ("tlh", r#"{"_meta": {"name": "tlhIngan Hol", "group": "Klingon"}, "Play": "yIDoH"}"#),
        ]);
        make_mod(&mods.0, "addon", "addon", &[("es-ES", r#"{"block.granite:granite": "Granito (complemento)", "block.other:thing": "Cosa"}"#)]);
        let (c, notes) = load_catalog(&game.0, &mods.0);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(c.lookup("es-ES", "Play"), Some("Jugar"));
        assert_eq!(c.lookup("en", "block.granite:granite"), Some("Granite"), "the mod's English text is the reference for its keys");
        // Folders load in name order, so "granite" overrides "addon" - not whatever the disk listed first.
        assert_eq!(c.lookup("es-ES", "block.granite:granite"), Some("Granito"));
        assert_eq!(c.lookup("es-ES", "block.other:thing"), Some("Cosa"), "a mod can ship another mod's keys");
        // A language only a mod knows is in the list, named by the mod.
        assert_eq!(c.language("tlh").unwrap().name, "tlhIngan Hol");
        assert!(c.groups().iter().any(|g| g.name == "Klingon"));
        // The mod's key counts in the percentages: Spanish has 2 of English's 2 keys.
        assert_eq!(c.percent("es-ES"), 100);
        assert_eq!(c.percent("tlh"), 50);
    }

    #[test]
    fn a_broken_language_file_is_a_note_not_a_crash() {
        let game = dir();
        write(&game.0.join("en.json"), r#"{"Play": "Play"}"#);
        write(&game.0.join("bad.json"), "{ not json");
        let (c, notes) = load_catalog(&game.0, &game.0.join("none"));
        assert_eq!(notes.len(), 1);
        assert!(!c.contains("bad"));
        assert_eq!(c.lookup("en", "Play"), Some("Play"));
    }

    #[test]
    fn selecting_a_language_needs_it_to_exist_and_reload_keeps_the_choice_when_it_can() {
        let game = dir();
        write(&game.0.join("es-ES.json"), r#"{"_meta": {"name": "Español"}}"#);
        let mut l = Languages::default();
        l.reload(&game.0, &game.0.join("none"));
        assert!(l.select("es-ES"));
        assert!(!l.select("es-ES"), "already chosen");
        assert!(!l.select("nope"));
        assert_eq!(l.code, "es-ES");
        std::fs::remove_file(game.0.join("es-ES.json")).unwrap();
        l.reload(&game.0, &game.0.join("none"));
        assert_eq!(l.code, "en", "a language that is gone falls back to English");
    }

    // ---- the files that ship ----

    fn shipped() -> (Catalog, Vec<String>) {
        load_catalog(Path::new(FOLDER), Path::new("no-mods-here"))
    }

    const EXPECTED: &[&str] = &[
        "ar", "en", "en-GB", "en-NZ", "en-IN", "en-ZA", "es-ES", "es-MX", "es-PR", "pt-PT", "pt-BR", "fr-FR", "hi", "zh-Hans", "zh-Hant", "zh-Hans-TW", "zh-Hant-TW",
        "zh-Hans-SG", "zh-Hant-SG", "yue-Hant", "yue-Hans",
    ];

    #[test]
    fn the_shipped_languages_all_load_cleanly_with_names_and_no_stray_keys() {
        let (c, notes) = shipped();
        assert!(notes.is_empty(), "{notes:?}");
        let english: HashSet<&str> = c.english_keys().into_iter().collect();
        assert!(english.len() > 250, "{} keys", english.len());
        for code in EXPECTED {
            let l = c.language(code).unwrap_or_else(|| panic!("{code} is missing"));
            assert_ne!(l.name, l.code, "{code} has no _meta name");
            for k in l.strings.keys() {
                // Arabic's extra plural forms (`key.few`...) belong to a key English has as `key.other`.
                let plural_of = k.rsplit_once('.').filter(|(_, f)| ["zero", "two", "few", "many"].contains(f)).map(|(b, _)| format!("{b}.other"));
                assert!(english.contains(k.as_str()) || plural_of.is_some_and(|o| english.contains(o.as_str())), "{code} has {k:?}, which English doesn't");
            }
        }
        for (code, fallback) in [
            ("es-MX", "es-ES"), ("es-PR", "es-MX"), ("pt-PT", "pt-BR"), ("zh-Hant-TW", "zh-Hant"), ("zh-Hans-TW", "zh-Hans"), ("zh-Hant-SG", "zh-Hant"),
            ("zh-Hans-SG", "zh-Hans"), ("yue-Hant", "zh-Hant"), ("yue-Hans", "zh-Hans"),
        ] {
            assert_eq!(c.language(code).unwrap().fallback.as_deref(), Some(fallback));
        }
        // Every Chinese language draws the ideograph set its script asks for.
        for code in EXPECTED.iter().filter(|c| c.starts_with("zh") || c.starts_with("yue")) {
            let want = if code.contains("Hant") { "zh-Hant" } else { "zh-Hans" };
            assert_eq!(c.language(code).unwrap().cjk.as_deref(), Some(want), "{code}");
        }
        // The groups the picker shows: English dialects together, Mandarin together, Cantonese on its own.
        let groups = c.groups();
        let members = |g: &str| groups.iter().find(|x| x.name == g).unwrap().members.clone();
        assert_eq!(members("English").len(), 5);
        assert_eq!(members("Español").len(), 3);
        assert_eq!(members("中文 (普通话)").len(), 6);
        assert_eq!(members("粵語 (廣東話)"), vec!["yue-Hans".to_string(), "yue-Hant".to_string()]);
        assert_eq!(members("العربية"), vec!["ar".to_string()]);
        assert!(c.language("ar").unwrap().rtl && !c.language("en").unwrap().rtl, "Arabic is marked right to left");
    }

    #[test]
    fn the_dialects_differ_where_their_speakers_would_and_inherit_the_rest() {
        let (c, _) = shipped();
        // Mandarin: mainland vocabulary in either script, Taiwan's own words in the Taiwanese variants.
        assert_eq!(c.lookup("zh-Hans", "Settings"), Some("设置"));
        assert_eq!(c.lookup("zh-Hant", "Settings"), Some("設置"), "traditional characters, mainland word");
        assert_eq!(c.lookup("zh-Hant-TW", "Settings"), Some("設定"));
        assert_eq!(c.lookup("zh-Hans-TW", "Settings"), Some("设定"), "Taiwan's word in simplified characters");
        assert_eq!(c.lookup("zh-Hant-SG", "Settings"), Some("設置"), "inherited");
        // The Taiwanese files hold only what differs.
        assert!(c.language("zh-Hant-TW").unwrap().strings.len() < 100);
        // Cantonese is written as Cantonese, not Mandarin in other characters.
        assert_eq!(c.lookup("zh-Hant", "You died!"), Some("你死了！"));
        assert_eq!(c.lookup("yue-Hant", "You died!"), Some("你死咗！"));
        assert_eq!(c.lookup("yue-Hans", "You died!"), Some("你死咗！"), "same words, simplified characters");
        assert_eq!(c.lookup("yue-Hans", "Settings"), Some("设定"));
        // Puerto Rican Spanish: its own words, then Mexico's, then Spain's.
        assert_eq!(c.lookup("es-PR", "Done"), Some("Listo"));
        assert_eq!(c.lookup("es-PR", "Settings"), Some("Configuración"), "from Mexican Spanish");
        assert_eq!(c.lookup("es-PR", "Worlds"), Some("Mundos"), "from Spain's");
        // English dialects: the reference text, with their few differences.
        assert_eq!(c.lookup("en-GB", "rarity.trash"), Some("Rubbish"));
        assert_eq!(c.lookup("en-IN", "rarity.trash"), Some("Trash"));
        assert_eq!(c.percent("en-NZ"), 100);
    }

    #[test]
    fn a_translation_keeps_every_placeholder_the_english_has() {
        let (c, _) = shipped();
        let holes = |s: &str| {
            let mut v: Vec<String> = Vec::new();
            let mut rest = s;
            while let Some(a) = rest.find('{') {
                let Some(b) = rest[a..].find('}') else { break };
                v.push(rest[a..a + b + 1].to_string());
                rest = &rest[a + b + 1..];
            }
            v.sort();
            v
        };
        for code in EXPECTED.iter().filter(|c| **c != "en") {
            for (k, v) in &c.language(code).unwrap().strings {
                // Extra plural forms have no English twin; a singular or dual form may drop the number ("one mod").
                let Some(en) = c.language("en").unwrap().strings.get(k) else { continue };
                if k.ends_with(".one") || k.ends_with(".two") {
                    continue;
                }
                assert_eq!(holes(v), holes(en), "{code}: {k:?}");
            }
        }
    }

    /// The percentages the game ships at. Everything shipped is fully translated; when strings are added to
    /// English and not yet translated (the translations are done when asked for), lower the affected entries
    /// here on purpose - the test then keeps them from slipping further.
    const SHIPPED_PERCENT: &[(&str, u8)] = &[
        ("ar", 100), ("en", 100), ("en-GB", 100), ("en-NZ", 100), ("en-IN", 100), ("en-ZA", 100), ("es-ES", 100), ("es-MX", 100), ("es-PR", 100), ("pt-PT", 100),
        ("pt-BR", 100), ("fr-FR", 100), ("hi", 100), ("zh-Hans", 100), ("zh-Hant", 100), ("zh-Hans-TW", 100), ("zh-Hant-TW", 100), ("zh-Hans-SG", 100),
        ("zh-Hant-SG", 100), ("yue-Hant", 100), ("yue-Hans", 100),
    ];

    #[test]
    fn the_shipped_languages_are_as_translated_as_they_claim() {
        let (c, _) = shipped();
        for (code, want) in SHIPPED_PERCENT {
            let got = c.percent(code);
            assert!(got >= *want, "{code} is at {got}%, expected at least {want}% (new English text needs translating, or lower SHIPPED_PERCENT on purpose)");
        }
    }

    /// Every key the code asks for must be in `en.json`, or it is missing from every percentage and from translators' lists.
    #[test]
    fn every_key_the_source_uses_is_in_english() {
        let (c, _) = shipped();
        let english: HashSet<String> = c.english_keys().into_iter().map(String::from).collect();
        let mut missing = Vec::new();
        let literal = r#""((?:[^"\\]|\\.)*)""#;
        let calls = regex_lite_find_all(literal);
        for entry in std::fs::read_dir("src").unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("lang.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let body = text.split("#[cfg(test)]\nmod tests").next().unwrap();
            for (name, first_literal_after) in calls.iter().map(|n| (n.as_str(), n)).map(|(n, _)| (n, ())) {
                let _ = first_literal_after;
                for found in literals_after_call(body, name) {
                    let key = found.replace("\\\"", "\"").replace("\\n", "\n");
                    let ok = if name.ends_with("tr_n(") { english.contains(&format!("{key}.other")) } else { english.contains(&key) };
                    if !ok {
                        missing.push(format!("{}: {key:?}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "keys used in code but missing from lang/en.json:\n{}", missing.join("\n"));
    }

    fn regex_lite_find_all(_: &str) -> Vec<String> {
        ["tr(", "tr_args(", "tr_n(", "crate::lang::tr(", "crate::lang::tr_args(", "crate::lang::tr_n("].iter().map(|s| s.to_string()).collect()
    }

    /// UI helpers that translate their text argument themselves (`ui_theme::button`, `settings::sbutton`...), so a
    /// literal given to one is a key too.
    const HELPERS: &[&str] = &[
        "sbutton(", "nav_button(", "choice(", "btn(", "dim(", "theme::hint(", "theme::label(", "theme::heading(", "theme::heading_with(",
        "theme::button(", "theme::badge(", "spawn_text_field(", "small(",
    ];

    /// Literals handed straight to a UI helper: the first literal in the call when only plain names come before it.
    fn helper_literals(body: &str, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = body;
        while let Some(at) = rest.find(name) {
            let before = rest[..at].chars().next_back();
            let after = &rest[at + name.len()..];
            rest = after;
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') && !name.contains("::") {
                continue;
            }
            let Some(q) = after.find('"') else { continue };
            let lead = &after[..q];
            if !lead.chars().all(|c| c.is_alphanumeric() || " _:.,&[]*".contains(c)) {
                continue; // something computed comes first: not a plain literal call
            }
            let bytes = after.as_bytes();
            let mut end = q + 1;
            while end < bytes.len() && !(bytes[end] == b'"' && bytes[end - 1] != b'\\') {
                end += 1;
            }
            out.push(after[q + 1..end].to_string());
        }
        out.sort();
        out.dedup();
        out
    }

    /// Text that is deliberately not a key: symbols, key names, mod-supplied or formatted text.
    fn is_not_text(s: &str) -> bool {
        s.chars().filter(|c| c.is_alphabetic()).count() < 2 || s.contains("{}") || s.starts_with(['[', '/'])
    }

    #[test]
    fn every_literal_given_to_a_ui_helper_is_in_english_too() {
        let (c, _) = shipped();
        let english: HashSet<String> = c.english_keys().into_iter().map(String::from).collect();
        let mut missing = Vec::new();
        for entry in std::fs::read_dir("src").unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let body = text.split("#[cfg(test)]\nmod tests").next().unwrap();
            for name in HELPERS {
                for lit in helper_literals(body, name) {
                    let key = lit.replace("\\\"", "\"").replace("\\n", "\n");
                    if !is_not_text(&key) && !english.contains(&key) {
                        missing.push(format!("{}: {key:?}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "text given to a UI helper but missing from lang/en.json:\n{}", missing.join("\n"));
    }

    /// The text of the first string literal directly after each `name` in `body` (`name("...")`).
    fn literals_after_call(body: &str, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = body;
        while let Some(at) = rest.find(name) {
            let before = rest[..at].chars().next_back();
            // `crate::lang::tr(` contains `tr(`: skip the inner match so each call counts once, and `str(` etc.
            let boundary = before.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            let after = &rest[at + name.len()..];
            if boundary && after.starts_with('"') {
                let mut end = 1;
                let bytes = after.as_bytes();
                while end < bytes.len() && !(bytes[end] == b'"' && bytes[end - 1] != b'\\') {
                    end += 1;
                }
                out.push(after[1..end].to_string());
            }
            rest = after;
        }
        out.sort();
        out.dedup();
        out
    }
}

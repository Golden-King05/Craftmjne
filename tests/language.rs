//! The active language is process-wide, so everything that changes it lives in this one test (a separate test
//! binary from the rest, and sequential inside) - parallel tests changing it would trip over each other.

use bevy::prelude::*;
use craftmjne::blocks::BlockRegistry;
use craftmjne::lang::{self, LanguagePlugin, Languages};
use craftmjne::mods::InstalledMods;
use craftmjne::save::SaveStore;

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("craftmjne-language-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app_with(store_dir: &std::path::Path, mods_dir: &std::path::Path) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(SaveStore::at(store_dir.to_path_buf()));
    app.insert_resource(InstalledMods(mods_dir.to_path_buf()));
    app.init_resource::<craftmjne::cjk::CjkStyle>();
    app.add_plugins(LanguagePlugin);
    app
}

/// One test, so the two scenarios never run at the same time.
#[test]
fn language_scenarios() {
    the_saved_language_is_what_the_game_speaks_and_switching_follows_through();
    a_mod_can_add_a_language_translate_itself_and_translate_another_mod();
}

fn the_saved_language_is_what_the_game_speaks_and_switching_follows_through() {
    let saves = temp("saves");
    let mods = temp("mods");
    // Nothing saved: English.
    let mut app = app_with(&saves, &mods);
    app.update();
    assert_eq!(lang::tr("Worlds"), "Worlds");
    assert_eq!(lang::active_code(), "en");

    // A saved choice is applied at startup.
    SaveStore::at(saves.clone()).save_language("pt-PT").unwrap();
    let mut app = app_with(&saves, &mods);
    app.update();
    assert_eq!(lang::active_code(), "pt-PT");
    assert_eq!(lang::tr("Settings"), "Definições", "Portugal's own word");
    assert_eq!(lang::tr("Worlds"), "Mundos", "inherited from the Brazilian text it falls back to");
    assert_eq!(lang::tr("Not a key anywhere"), "Not a key anywhere", "an unknown key is shown as it is");

    // Plurals follow the language: Portugal's Portuguese has no singular for 0, Brazil's does.
    assert_eq!(lang::tr_n("count.mods", 0, "{n} mod", "{n} mods", &[]), "0 mods");
    assert_eq!(lang::tr_n("count.mods", 1, "{n} mod", "{n} mods", &[]), "1 mod");

    // Content names come through the registry's own display helper.
    let registry = BlockRegistry::with_defaults();
    assert_eq!(registry.def(registry.id("grass")).display(), "Relva");

    // Switching live: the resource changes, the plugin's system installs it.
    {
        let mut languages = app.world_mut().resource_mut::<Languages>();
        assert!(languages.select("es-MX"));
    }
    app.update();
    assert_eq!(lang::tr("Settings"), "Configuración", "México's own word");
    assert_eq!(lang::tr("Worlds"), "Mundos", "and España's for the rest");
    assert_eq!(registry.def(registry.id("planks")).display(), "Tablas de madera");

    // Chinese: the variant picks the ideograph set the font draws.
    {
        let mut languages = app.world_mut().resource_mut::<Languages>();
        assert!(languages.select("zh-Hant"));
    }
    app.update();
    assert_eq!(lang::tr("Settings"), "設定");
    assert_eq!(app.world().resource::<craftmjne::cjk::CjkStyle>().0, craftmjne::cjk::Style::TraditionalChinese);
    app.world_mut().resource_mut::<Languages>().select("zh-Hans");
    app.update();
    assert_eq!(lang::tr("Settings"), "设置");
    assert_eq!(app.world().resource::<craftmjne::cjk::CjkStyle>().0, craftmjne::cjk::Style::Chinese);

    // Back to English: nothing of the other language is left behind.
    app.world_mut().resource_mut::<Languages>().select("en");
    app.update();
    assert_eq!(lang::tr("Settings"), "Settings");
    assert_eq!(registry.def(registry.id("grass")).display(), "Grass");
}

fn a_mod_can_add_a_language_translate_itself_and_translate_another_mod() {
    let saves = temp("saves2");
    let mods = temp("mods2");
    let put = |path: std::path::PathBuf, text: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    // A mod with its own content and English text, a Spanish version, and a language the game doesn't have.
    put(mods.join("granite/mod.json"), r#"{"id": "granite"}"#);
    put(mods.join("granite/lang/en.json"), r#"{"block.granite:granite": "Granite"}"#);
    put(mods.join("granite/lang/es-ES.json"), r#"{"block.granite:granite": "Granito"}"#);
    put(mods.join("granite/lang/gl.json"), r#"{"_meta": {"name": "Galego", "group": "Galego", "fallback": "es-ES"}, "Worlds": "Mundos galegos"}"#);
    // An add-on that only translates the first mod's name into French.
    put(mods.join("granite_fr/mod.json"), r#"{"id": "granite_fr"}"#);
    put(mods.join("granite_fr/lang/fr-FR.json"), r#"{"block.granite:granite": "Granite (fr)"}"#);

    SaveStore::at(saves.clone()).save_language("gl").unwrap();
    let mut app = app_with(&saves, &mods);
    app.update();
    // The new language is chosen and speaks: its own word, then Spanish, then English.
    assert_eq!(lang::active_code(), "gl");
    assert_eq!(lang::tr("Worlds"), "Mundos galegos");
    assert_eq!(lang::tr("Settings"), "Ajustes", "inherited through its fallback");
    assert_eq!(lang::name_of("block", "granite:granite", "Granite"), "Granito");
    let languages = app.world().resource::<Languages>();
    assert!(languages.catalog.groups().iter().any(|g| g.name == "Galego"), "it is in the list");
    // The add-on's French name for a block of another mod works too.
    app.world_mut().resource_mut::<Languages>().select("fr-FR");
    app.update();
    assert_eq!(lang::name_of("block", "granite:granite", "Granite"), "Granite (fr)");
    // The mod's English key is part of what every percentage is measured against: Galego inherits it from
    // Spanish, French gets it from the add-on, and Portuguese has nobody translating it - so it drops below 100.
    let languages = app.world().resource::<Languages>();
    assert_eq!(languages.catalog.percent("gl"), 100);
    assert_eq!(languages.catalog.percent("fr-FR"), 100);
    assert!(languages.catalog.percent("pt-BR") < 100, "an untranslated mod string lowers the percentage on its own");
    lang::install(std::sync::Arc::new(lang::Catalog::default()), "en");
}
